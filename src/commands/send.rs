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
    tx::{Fees, generator::summary::GeneratorSummary, PaymentDestination, PaymentOutputs},
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
    payload: Option<String>,
    interactive: bool,
    no_confirmation: bool,
) -> Result<()> {
    init_storage(&network_id)?;

    // Parse and validate priority fee early — before any network connection
    let priority_fee_sompi: i64 = match priority_fee {
        Some(ref s) => kaspa_wallet_core::utils::try_kaspa_str_to_sompi_i64(s)
            .context("Invalid priority fee")?
            .unwrap_or(0),
        None => 0,
    };
    const MAX_PRIORITY_FEE_SOMPI: i64 = 10_000_000_000; // 100 KAS
    if priority_fee_sompi > MAX_PRIORITY_FEE_SOMPI {
        anyhow::bail!(
            "Priority fee {} exceeds the maximum allowed (100 KAS). Aborting.",
            sompi_to_kaspa_string_with_suffix(priority_fee_sompi as u64, &network_id.network_type)
        );
    }

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

    let outputs = PaymentOutputs::from((address.clone(), amount_sompi));
    let abortable = Abortable::default();
    let explorer = explorer_base(&network_id);

    // Parse payload: 0x-prefixed → hex binary, otherwise → UTF-8 bytes
    let tx_payload: Option<Vec<u8>> = match payload {
        None => None,
        Some(ref s) if s.starts_with("0x") || s.starts_with("0X") => {
            let hex_str = &s[2..];
            let bytes = (0..hex_str.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex_str[i..i + 2], 16))
                .collect::<Result<Vec<u8>, _>>()
                .context("Invalid hex payload")?;
            Some(bytes)
        }
        Some(ref s) => Some(s.as_bytes().to_vec()),
    };

    // --- Estimate ---
    let estimate = account
        .clone()
        .estimate(
            PaymentDestination::from(outputs.clone()),
            None,
            Fees::from(priority_fee_sompi as i64),
            tx_payload.clone(),
            &abortable,
        )
        .await
        .context("Failed to estimate transaction")?;

    let total_sompi = amount_sompi + estimate.aggregate_fees;
    let fees_str = sompi_to_kaspa_string_with_suffix(estimate.aggregate_fees, &network_id.network_type);
    let fees_display = if priority_fee_sompi > 1_000_000_000 {
        // > 10 KAS priority fee: highlight in bright orange
        format!("\x1b[38;5;208m{}\x1b[0m", fees_str)
    } else {
        fees_str
    };
    println!(
        "Amount : {}",
        sompi_to_kaspa_string_with_suffix(amount_sompi, &network_id.network_type)
    );
    println!("Fees   : {}", fees_display);
    println!(
        "Total  : {}",
        sompi_to_kaspa_string_with_suffix(total_sompi, &network_id.network_type)
    );
    println!("UTXOs  : {} ({} transaction(s))", estimate.aggregated_utxos, estimate.number_of_generated_transactions);
    println!("To     : {}", address);
    println!();

    if !no_confirmation {
        if interactive {
            use std::io::{Write, BufRead};
            // Extra dedicated warning + confirmation when priority fee is unusually high (> 10 KAS)
            if priority_fee_sompi > 1_000_000_000 {
                println!(
                    "\x1b[38;5;208m⚠  Priority fee is unusually high ({}).\x1b[0m",
                    sompi_to_kaspa_string_with_suffix(priority_fee_sompi as u64, &network_id.network_type)
                );
                print!("This fee seems excessive. Are you sure? [y/N]: ");
                std::io::stdout().flush().ok();
                let mut warn_line = String::new();
                std::io::stdin().lock().read_line(&mut warn_line).context("Failed to read input")?;
                if !warn_line.trim().eq_ignore_ascii_case("y") {
                    println!("Aborted.");
                    wallet.stop().await.ok();
                    return Ok(());
                }
                println!();
            }
            print!("Confirm? [y/N]: ");
            std::io::stdout().flush().ok();
            let mut line = String::new();
            std::io::stdin().lock().read_line(&mut line).context("Failed to read input")?;
            if !line.trim().eq_ignore_ascii_case("y") {
                println!("Aborted.");
                wallet.stop().await.ok();
                return Ok(());
            }
            println!();
        } else {
            use std::io::Write;
            for i in (1u8..=9).rev() {
                print!("\rSending in {}... (Ctrl+C to abort)", i);
                std::io::stdout().flush().ok();
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            println!("\rSending...        ");
            println!();
        }
    }

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
            tx_payload,
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
