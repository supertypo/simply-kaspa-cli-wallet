use std::sync::Arc;

use anyhow::{Context, Result};
use kaspa_consensus_core::network::NetworkId;
use kaspa_wallet_core::{
    account::Account,
    api::{
        message::{AccountsGetUtxosRequest, WalletOpenRequest},
        traits::WalletApi,
    },
    deterministic::AccountId,
    events::Events,
    tx::{generator::summary::GeneratorSummary, PaymentOutputs},
    utils::{sompi_to_kaspa_string_with_suffix, try_kaspa_str_to_sompi},
    prelude::Address,
};
use kaspa_wallet_keys::secret::Secret;
use workflow_core::{abortable::Abortable, channel::MultiplexerChannel};

use crate::wallet::{build_wallet, explorer_base, init_storage, resolve_url};

pub async fn run(
    network_id: NetworkId,
    rpc_url: Option<String>,
    wallet_name: String,
    password: String,
    to_address: String,
    amount: String,
    priority_fee: Option<String>,
) -> Result<()> {
    init_storage(&network_id)?;

    let rpc_url = resolve_url(rpc_url, network_id).await?;
    let wallet = build_wallet(rpc_url.clone(), network_id)?;

    let events: MultiplexerChannel<Box<Events>> = wallet.multiplexer().channel();

    wallet.start().await.context("Failed to start wallet")?;

    wallet
        .clone()
        .connect(rpc_url.clone(), &network_id)
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

    {
        let guard = wallet.guard();
        let lock = guard.lock().await;
        wallet
            .activate_accounts(None, &lock)
            .await
            .context("Failed to activate accounts")?;
    }

    // Wait for UTXO scan to start
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

    let account: Arc<dyn Account> = wallet
        .active_accounts()
        .first()
        .context("No active accounts found")?;

    let account_name = account.name().unwrap_or_else(|| "default".to_string());

    // Get balance/UTXOs for the header
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

    println!("Wallet : {}", wallet_name);
    println!("Network: {}", network_id);
    println!("Node   : {}", rpc_url.as_deref().unwrap_or("-"));
    println!();
    println!("  Account : {}", account_name);
    println!(
        "  Balance : {}",
        sompi_to_kaspa_string_with_suffix(total_balance, &network_id.network_type)
    );
    println!("  UTXOs   : {}", total_utxos);
    println!();

    // Parse destination and amount
    let address = Address::try_from(to_address.as_str())
        .with_context(|| format!("Invalid destination address: {}", to_address))?;

    let amount_sompi = try_kaspa_str_to_sompi(&amount)
        .context("Invalid amount")?
        .filter(|&v| v > 0)
        .with_context(|| format!("Amount must be greater than zero: {}", amount))?;

    let priority_fee_sompi: i64 = match priority_fee {
        Some(ref s) => kaspa_wallet_core::utils::try_kaspa_str_to_sompi_i64(s)
            .context("Invalid priority fee")?
            .unwrap_or(0),
        None => 0,
    };

    let outputs = PaymentOutputs::from((address.clone(), amount_sompi));
    let abortable = Abortable::default();
    let explorer = explorer_base(&network_id);

    println!("Sending {} to {}", sompi_to_kaspa_string_with_suffix(amount_sompi, &network_id.network_type), address);
    println!();
    println!("Transactions:");

    let notifier: kaspa_wallet_core::account::GenerationNotifier = Arc::new(move |tx| {
        let id = tx.id();
        if let Some(base) = &explorer {
            println!("   {}/transactions/{}", base, id);
        } else {
            println!("   {}", id);
        }
    });

    let (summary, _tx_ids): (GeneratorSummary, Vec<_>) = account
        .send(
            outputs.into(),
            None,
            priority_fee_sompi.into(),
            None,
            wallet_secret,
            None,
            &abortable,
            Some(notifier),
        )
        .await
        .context("Send failed")?;

    println!();
    println!(
        "Sent {} to {}",
        sompi_to_kaspa_string_with_suffix(amount_sompi, &network_id.network_type),
        address,
    );
    println!(
        "Fees: {}",
        sompi_to_kaspa_string_with_suffix(summary.aggregate_fees, &network_id.network_type)
    );
    println!();

    wallet.stop().await.ok();
    Ok(())
}
