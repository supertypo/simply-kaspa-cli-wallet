use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use kaspa_consensus_core::network::NetworkId;
use kaspa_wallet_core::{
    account::Account,
    api::{message::{AccountsGetUtxosRequest, WalletOpenRequest}, traits::WalletApi},
    deterministic::AccountId,
    events::Events,
    tx::generator::summary::GeneratorSummary,
    utils::sompi_to_kaspa_string_with_suffix,
};
use kaspa_wallet_keys::secret::Secret;
use workflow_core::{abortable::Abortable, channel::MultiplexerChannel};

use crate::wallet::{build_wallet, explorer_base, init_storage, resolve_url};

pub async fn run(
    network_id: NetworkId,
    rpc_url: Option<String>,
    wallet_name: String,
    password: String,
) -> Result<()> {
    init_storage(&network_id)?;

    let rpc_url = resolve_url(rpc_url, network_id).await?;
    let wallet = build_wallet(rpc_url.clone(), network_id)?;

    // Subscribe to events before starting
    let events: MultiplexerChannel<Box<Events>> = wallet.multiplexer().channel();

    wallet.start().await.context("Failed to start wallet")?;

    wallet.clone().connect(rpc_url.clone(), &network_id)
        .await
        .context("Failed to connect to node")?;

    let wallet_secret = Secret::new(password.into_bytes());

    let open_resp = wallet
        .clone()
        .wallet_open_call(WalletOpenRequest {
            wallet_secret: wallet_secret.clone(),
            filename: Some(wallet_name.clone()),
            account_descriptors: true,
            legacy_accounts: None,
        })
        .await
        .context("Failed to open wallet")?;

    let account_ids: Vec<AccountId> = open_resp
        .account_descriptors
        .unwrap_or_default()
        .iter()
        .map(|d| d.account_id)
        .collect();

    // Activate accounts so UTXO entries are loaded
    {
        let guard = wallet.guard();
        let lock = guard.lock().await;
        wallet
            .activate_accounts(None, &lock)
            .await
            .context("Failed to activate accounts")?;
    }

    // Wait for all accounts to report a Balance event (UTXO scan started)
    let mut seen: std::collections::HashSet<AccountId> = std::collections::HashSet::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        match tokio::time::timeout(remaining, events.recv()).await {
            Err(_) | Ok(Err(_)) => break,
            Ok(Ok(boxed)) => match *boxed {
                Events::Balance { id, .. } => {
                    seen.insert(AccountId::from(id));
                    if account_ids.iter().all(|id| seen.contains(id)) {
                        break;
                    }
                }
                _ => {}
            },
        }
    }

    println!("Wallet : {}", wallet_name);
    println!("Network: {}", network_id);
    println!("Node   : {}", rpc_url.unwrap_or_default());
    println!();

    let abortable = Abortable::default();
    let account: Arc<dyn Account> = wallet
        .active_accounts()
        .first()
        .context("No active accounts found")?;

    let account_name = account.name().unwrap_or_else(|| "default".to_string());

    // Get total UTXO count and balance before sweeping
    let utxo_resp = wallet
        .clone()
        .accounts_get_utxos_call(AccountsGetUtxosRequest {
            account_id: *account.id(),
            addresses: None,
            min_amount_sompi: None,
        })
        .await
        .context("Failed to get UTXOs")?;
    let total_utxos = utxo_resp.utxos.len();
    let total_balance: u64 = utxo_resp.utxos.iter().map(|u| u.amount).sum();

    println!("  Account : {}", account_name);
    println!("  Balance : {}", sompi_to_kaspa_string_with_suffix(total_balance, &network_id.network_type));
    println!("  UTXOs   : {}", total_utxos);
    println!();

    let explorer = explorer_base(&network_id);

    // Running counter: remaining = previous - inputs_consumed + 1 (change output)
    let remaining = Arc::new(Mutex::new(total_utxos));
    let remaining_clone = remaining.clone();

    let explorer_clone = explorer.clone();
    let notifier: kaspa_wallet_core::account::GenerationNotifier = Arc::new(move |tx| {
        let inputs = tx.utxo_entries().len();
        let r = {
            let mut r = remaining_clone.lock().unwrap();
            *r = r.saturating_sub(inputs) + 1;
            *r
        };
        let id = tx.id();
        // suppress suffix on final tx(s) — remaining==1 means only the consolidated output is left
        let suffix = if r > 1 {
            format!(" ({} UTXOs remaining)", r)
        } else {
            String::new()
        };
        if let Some(base) = &explorer_clone {
            println!("   {}/transactions/{}{}", base, id, suffix);
        } else {
            println!("   {}{}", id, suffix);
        }
    });

    println!("Transactions:");
    let (summary, tx_ids): (GeneratorSummary, Vec<_>) = account
        .sweep(wallet_secret, None, None, &abortable, Some(notifier))
        .await
        .context("Sweep failed")?;

    if tx_ids.is_empty() {
        println!("Nothing to sweep — wallet already has a single UTXO or is empty.");
    } else {
        println!();
        let fees = sompi_to_kaspa_string_with_suffix(summary.aggregate_fees, &network_id.network_type);
        println!(
            "Swept {} UTXOs across {} transaction(s). Fees: {}",
            summary.aggregated_utxos,
            summary.number_of_generated_transactions,
            fees
        );
    }

    wallet.stop().await.ok();
    Ok(())
}
