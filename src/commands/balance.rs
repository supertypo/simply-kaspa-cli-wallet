use std::{collections::HashMap, sync::Arc};

use anyhow::{Context, Result};
use kaspa_consensus_core::network::NetworkId;
#[allow(unused_imports)]
use kaspa_rpc_core::api::rpc::RpcApi as _;
use kaspa_wallet_core::{
    api::{
        message::{AccountsEnumerateRequest, AccountsGetUtxosRequest, WalletOpenRequest},
        traits::WalletApi,
    },
    deterministic::AccountId,
    events::Events,
    prelude::Address,
    utils::sompi_to_kaspa_string_with_suffix,
};
use kaspa_wallet_keys::secret::Secret;
use workflow_core::channel::MultiplexerChannel;

use crate::wallet::{build_wallet, init_storage, resolve_url};

pub async fn run(
    network_id: NetworkId,
    rpc_url: Option<String>,
    wallet_name: String,
    password: String,
) -> Result<()> {
    init_storage(&network_id)?;

    // Resolve the URL first — when none is given, ask the public resolver.
    // This is required because try_with_wrpc hard-codes a localhost ctor_url
    // that would otherwise shadow the resolver inside connect(None).
    let rpc_url = resolve_url(rpc_url, network_id).await?;

    let wallet = build_wallet(rpc_url.clone(), network_id)?;
    let wallet_arc = Arc::clone(&wallet);

    // Subscribe to events before starting so we don't miss any
    let events: MultiplexerChannel<Box<Events>> = wallet.multiplexer().channel();

    wallet.start().await.context("Failed to start wallet")?;

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

    let descriptors = open_resp.account_descriptors.unwrap_or_default();

    // Activate all accounts to trigger UTXO scan and balance events
    {
        let guard = wallet.guard();
        let lock = guard.lock().await;
        wallet_arc
            .activate_accounts(None, &lock)
            .await
            .context("Failed to activate accounts")?;
    }

    // Wait for every account to report at least one Balance event so the UTXO scan has started.
    let account_ids: Vec<AccountId> = descriptors.iter().map(|d| d.account_id).collect();
    let mut seen: std::collections::HashSet<AccountId> = std::collections::HashSet::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);

    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let event = tokio::time::timeout(remaining, events.recv()).await;
        match event {
            Ok(Ok(boxed)) => match *boxed {
                Events::Balance { id, .. } => {
                    seen.insert(AccountId::from(id));
                    if account_ids.iter().all(|id| seen.contains(id)) {
                        break;
                    }
                }
                Events::Error { message } => {
                    eprintln!("Wallet error: {}", message);
                    break;
                }
                _ => {}
            },
            Err(_) | Ok(Err(_)) => break,
        }
    }

    // Get fresh descriptors after scan — these have updated derivation indices and full address lists
    let fresh_descriptors = wallet_arc
        .clone()
        .accounts_enumerate_call(AccountsEnumerateRequest {})
        .await
        .context("Failed to enumerate accounts")?
        .account_descriptors;

    // Fetch per-address UTXO breakdown before stopping
    struct AddrRow {
        balance: u64,
        pending: u64,
    }
    let mut address_utxos: HashMap<AccountId, Vec<(Address, AddrRow)>> = HashMap::new();
    let mut utxo_counts: HashMap<AccountId, usize> = HashMap::new();
    {
        for descriptor in &fresh_descriptors {
            // Mature UTXOs from wallet (already scanned, spendable)
            let resp = wallet_arc
                .clone()
                .accounts_get_utxos_call(AccountsGetUtxosRequest {
                    account_id: descriptor.account_id,
                    addresses: None,
                    min_amount_sompi: None,
                })
                .await
                .context("Failed to get UTXOs")?;

            let mut mature_by_addr: HashMap<String, u64> = HashMap::new();
            let mut addr_obj: HashMap<String, Address> = HashMap::new();
            for utxo in resp.utxos {
                if let Some(addr) = utxo.address {
                    *mature_by_addr.entry(addr.to_string()).or_default() += utxo.amount;
                    addr_obj.entry(addr.to_string()).or_insert(addr);
                }
            }

            // All UTXOs from the node for the addresses we already know from mature UTXOs.
            // Subtracting mature gives us pending (immature coinbase) per address.
            let known_addresses: Vec<Address> = addr_obj.values().cloned().collect();
            let rpc_entries = if !known_addresses.is_empty() {
                wallet
                    .rpc_api()
                    .get_utxos_by_addresses(known_addresses)
                    .await
                    .context("Failed to get UTXOs from node")?
            } else {
                vec![]
            };

            utxo_counts.insert(descriptor.account_id, rpc_entries.len());

            let mut rpc_total_by_addr: HashMap<String, u64> = HashMap::new();
            for entry in &rpc_entries {
                if let Some(addr) = &entry.address {
                    *rpc_total_by_addr.entry(addr.to_string()).or_default() +=
                        entry.utxo_entry.amount;
                    addr_obj
                        .entry(addr.to_string())
                        .or_insert_with(|| addr.clone());
                }
            }

            // Union of all known addresses (some may only have pending)
            let all_addr_keys: std::collections::HashSet<String> = mature_by_addr
                .keys()
                .chain(rpc_total_by_addr.keys())
                .cloned()
                .collect();

            let mut rows: Vec<(Address, AddrRow)> = all_addr_keys
                .into_iter()
                .filter_map(|k| {
                    let addr = addr_obj.get(&k)?.clone();
                    let mature = *mature_by_addr.get(&k).unwrap_or(&0);
                    let rpc_total = *rpc_total_by_addr.get(&k).unwrap_or(&0);
                    let pending = rpc_total.saturating_sub(mature);
                    if mature == 0 && pending == 0 {
                        return None;
                    }
                    Some((
                        addr,
                        AddrRow {
                            balance: mature,
                            pending,
                        },
                    ))
                })
                .collect();
            rows.sort_by(|a, b| b.1.balance.cmp(&a.1.balance));
            address_utxos.insert(descriptor.account_id, rows);
        }
    }

    events.close();
    wallet.stop().await.context("Failed to stop wallet")?;

    // --- Output ---
    let network_type = network_id.network_type;

    println!("Wallet : {}", wallet_name);
    println!("Network: {}", network_id);
    println!("Node   : {}", rpc_url.as_deref().unwrap_or("unknown"));
    println!();

    for descriptor in &fresh_descriptors {
        let name = descriptor.account_name.as_deref().unwrap_or("(unnamed)");
        let rows = address_utxos.get(&descriptor.account_id);
        // Derive totals from the RPC-based per-address data so account summary and
        // per-address breakdown are always consistent (same point in time).
        let mature: u64 = rows
            .map(|r| r.iter().map(|(_, row)| row.balance).sum())
            .unwrap_or(0);
        let pending: u64 = rows
            .map(|r| r.iter().map(|(_, row)| row.pending).sum())
            .unwrap_or(0);
        let utxo_count = utxo_counts
            .get(&descriptor.account_id)
            .copied()
            .unwrap_or(0);
        println!("  Account : {}", name);
        println!(
            "  Balance : {}",
            sompi_to_kaspa_string_with_suffix(mature, &network_type)
        );
        if pending > 0 {
            println!(
                "  Pending : {}",
                sompi_to_kaspa_string_with_suffix(pending, &network_type)
            );
        }
        println!("  UTXOs   : {}", utxo_count);
        println!();

        if let Some(rows) = address_utxos.get(&descriptor.account_id) {
            let addr_col = 72;
            let has_pending = rows.iter().any(|(_, r)| r.pending > 0);
            if has_pending {
                println!(
                    "  {:<addr_col$}  {:>26}  {:>26}",
                    "Address", "Balance", "Pending"
                );
                println!("  {}", "-".repeat(addr_col + 57));
                for (addr, row) in rows {
                    let bal_str = sompi_to_kaspa_string_with_suffix(row.balance, &network_type);
                    let pend_str = if row.pending > 0 {
                        sompi_to_kaspa_string_with_suffix(row.pending, &network_type)
                    } else {
                        String::new()
                    };
                    println!(
                        "  {:<addr_col$}  {:>26}  {:>26}",
                        addr.to_string(),
                        bal_str,
                        pend_str
                    );
                }
            } else {
                println!("  {:<addr_col$}  {:>26}", "Address", "Balance");
                println!("  {}", "-".repeat(addr_col + 29));
                for (addr, row) in rows {
                    let bal_str = sompi_to_kaspa_string_with_suffix(row.balance, &network_type);
                    println!("  {:<addr_col$}  {:>26}", addr.to_string(), bal_str);
                }
            }
            println!();
        }
    }

    Ok(())
}
