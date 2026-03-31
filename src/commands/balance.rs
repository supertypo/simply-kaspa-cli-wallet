use std::{collections::HashMap, sync::Arc};

use anyhow::{Context, Result};
use kaspa_consensus_core::network::NetworkId;
use kaspa_wallet_core::{
    api::{message::WalletOpenRequest, traits::WalletApi},
    deterministic::AccountId,
    events::Events,
    utxo::balance::Balance,
    utils::sompi_to_kaspa_string_with_suffix,
};
use kaspa_wallet_keys::secret::Secret;
use workflow_core::channel::MultiplexerChannel;

use crate::wallet::{build_wallet, init_storage};

pub async fn run(
    network_id: NetworkId,
    rpc_url: Option<String>,
    wallet_name: String,
    verbose: bool,
    password: String,
) -> Result<()> {
    init_storage(&network_id)?;

    let wallet = build_wallet(rpc_url.clone(), network_id)?;
    let wallet_arc = Arc::clone(&wallet);

    // Subscribe to events before starting so we don't miss any
    let events: MultiplexerChannel<Box<Events>> = wallet.multiplexer().channel();

    wallet.start().await.context("Failed to start wallet")?;

    // Connect to node (blocks until node is synced).
    // Pass the explicit URL when given; otherwise the resolver picks a public node.
    wallet_arc
        .clone()
        .connect(rpc_url.clone(), &network_id)
        .await
        .context("Failed to connect to node")?;

    let wallet_secret = Secret::new(password.into_bytes());

    // Open wallet and get account descriptors
    let open_resp = wallet_arc
        .clone()
        .wallet_open_call(WalletOpenRequest {
            wallet_secret,
            filename: Some(wallet_name.clone()),
            account_descriptors: true,
            legacy_accounts: None,
        })
        .await
        .context("Failed to open wallet")?;

    let descriptors = open_resp
        .account_descriptors
        .unwrap_or_default();

    // Activate all accounts to trigger UTXO scan and balance events
    {
        let guard = wallet.guard();
        let lock = guard.lock().await;
        wallet_arc
            .activate_accounts(None, &lock)
            .await
            .context("Failed to activate accounts")?;
    }

    // Collect balances from events.
    // Strategy: keep updating on every Balance event; once all accounts have reported
    // at least once, start a 2-second settle window. If no new Balance arrives within
    // that window, the UTXO scan is done and the values are stable.
    let account_ids: Vec<AccountId> = descriptors.iter().map(|d| d.account_id).collect();
    let mut balances: HashMap<AccountId, Balance> = HashMap::new();

    let hard_deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let settle_duration = std::time::Duration::from_secs(2);
    let mut settle_deadline: Option<std::time::Instant> = None;

    loop {
        let now = std::time::Instant::now();

        // If settle window has elapsed, we're done.
        if let Some(sd) = settle_deadline {
            if now >= sd {
                break;
            }
        }

        if now >= hard_deadline {
            break;
        }

        // Wake up when settle window expires, or hard deadline — whichever is sooner.
        let wait_until = settle_deadline.unwrap_or(hard_deadline).min(hard_deadline);
        let remaining = wait_until.saturating_duration_since(now);

        let event = tokio::time::timeout(remaining, events.recv()).await;

        match event {
            Ok(Ok(boxed)) => match *boxed {
                Events::Balance { balance, id } => {
                    let acct_id = AccountId::from(id);
                    if account_ids.contains(&acct_id) {
                        balances.insert(acct_id, balance.unwrap_or_default());
                        // Once every account has reported at least once, open settle window.
                        // Reset it on each subsequent update so we keep waiting while the scan
                        // is still streaming results.
                        if account_ids.iter().all(|id| balances.contains_key(id)) {
                            settle_deadline =
                                Some(std::time::Instant::now() + settle_duration);
                        }
                    }
                }
                Events::Error { message } => {
                    eprintln!("Wallet error: {}", message);
                    break;
                }
                _ => {}
            },
            // Timeout means settle window elapsed with no new events — done.
            Err(_timeout) => break,
            Ok(Err(_)) => break,
        }
    }

    events.close();
    wallet.stop().await.context("Failed to stop wallet")?;

    // --- Output ---
    let network_type = network_id.network_type;

    let total_mature: u64 = balances.values().map(|b| b.mature).sum();
    let total_pending: u64 = balances.values().map(|b| b.pending).sum();

    println!("Wallet : {}", wallet_name);
    println!("Network: {}", network_id);
    println!();

    if verbose {
        for descriptor in &descriptors {
            let name = descriptor.account_name.as_deref().unwrap_or("(unnamed)");
            let balance = balances.get(&descriptor.account_id);
            let mature = balance.map(|b| b.mature).unwrap_or(0);
            let pending = balance.map(|b| b.pending).unwrap_or(0);
            println!("  Account : {}", name);
            println!("  Balance : {}", sompi_to_kaspa_string_with_suffix(mature, &network_type));
            if pending > 0 {
                println!("  Pending : {}", sompi_to_kaspa_string_with_suffix(pending, &network_type));
            }
            if let Some(addr) = &descriptor.receive_address {
                println!("  Address : {}", addr);
            }
            println!();
        }
    }

    println!(
        "Total balance : {}",
        sompi_to_kaspa_string_with_suffix(total_mature, &network_type)
    );
    if total_pending > 0 {
        println!(
            "Total pending : {}",
            sompi_to_kaspa_string_with_suffix(total_pending, &network_type)
        );
    }

    Ok(())
}
