use std::sync::Arc;

use anyhow::{Context, Result};
use kaspa_consensus_core::network::NetworkId;
use kaspa_wallet_core::wallet::Wallet;
use kaspa_wrpc_client::{resolver::Resolver, WrpcEncoding};

/// Initialize local storage under `~/.simply-kaspa-cli-wallet/<network>/`.
///
/// # Safety
/// Must be called once before any wallet operations, from the main thread.
pub fn init_storage(network: &NetworkId) -> Result<()> {
    let home = home_dir().context("Cannot determine home directory")?;
    let folder = format!("{}/{}/{}", home, ".simply-kaspa-cli-wallet", network);

    // SAFETY: called once, before wallet initialisation, from the main thread.
    unsafe {
        kaspa_wallet_core::storage::local::set_default_storage_folder(folder)
            .context("Failed to set storage folder")?;
    }
    Ok(())
}

/// Build an `Arc<Wallet>` wired to the given network.
/// `rpc_url` must already be resolved to a concrete URL (use `resolve_url` first).
pub fn build_wallet(rpc_url: Option<String>, network_id: NetworkId) -> Result<Arc<Wallet>> {
    let storage = Wallet::local_store().context("Failed to create local store")?;
    let resolver = Resolver::default();

    let wallet = Wallet::try_with_wrpc(storage, Some(resolver), Some(network_id))
        .context("Failed to create wallet")?;

    let wallet = if let Some(url) = rpc_url {
        wallet.with_url(Some(&url))
    } else {
        wallet
    };

    Ok(Arc::new(wallet.with_network_id(network_id)))
}

/// Resolve a concrete wRPC URL for the given network.
///
/// Strategy:
/// 1. Explicit URL given → return it.
/// 2. testnet-N → DNS seeder `tnN-dnsseed.kasia.fyi`; try each returned IPv4 address
///    with a 5 s TCP probe in order; return first reachable node as `ws://ip:port`.
/// 3. Everything else (mainnet, devnet, …) → public Kaspa resolver.
pub async fn resolve_url(
    rpc_url: Option<String>,
    network_id: NetworkId,
) -> Result<Option<String>> {
    if rpc_url.is_some() {
        return Ok(rpc_url);
    }

    // DNS seeder path for testnet-12
    if network_id.network_type == kaspa_consensus_core::network::NetworkType::Testnet
        && network_id.suffix == Some(12)
    {
        let port = network_id.network_type.default_borsh_rpc_port();
        let url = dns_seeder_resolve("tn12-dnsseed.kasia.fyi", port)
            .await
            .context("DNS seeder for testnet-12 returned no reachable nodes")?;
        return Ok(Some(url));
    }

    // Public resolver fallback (mainnet, devnet, simnet, or testnet when DNS seeder fails)
    let resolver = Resolver::default();
    let url = resolver.get_url(WrpcEncoding::Borsh, network_id)
        .await
        .context("Resolver failed to find a public node")?;
    Ok(Some(url))
}

/// DNS-seeder based resolution: look up A records for `seeder_host`, then TCP-probe
/// each IPv4 address on `port` with a 5 s timeout, returning the first reachable node.
async fn dns_seeder_resolve(seeder_host: &str, port: u16) -> Result<String> {
    let probe_addr = format!("{}:{}", seeder_host, port);

    let addrs: Vec<std::net::SocketAddr> =
        tokio::net::lookup_host(&probe_addr).await.context(format!("DNS lookup failed for {}", seeder_host))?.filter(|a| a.is_ipv4()).collect();

    if addrs.is_empty() {
        anyhow::bail!("DNS seeder {} returned no IPv4 addresses", seeder_host);
    }

    let probe_timeout = std::time::Duration::from_secs(3);
    for addr in &addrs {
        if tokio::time::timeout(probe_timeout, tokio::net::TcpStream::connect(addr)).await.is_ok_and(|r| r.is_ok()) {
            return Ok(format!("ws://{}:{}", addr.ip(), port));
        }
    }

    anyhow::bail!("DNS seeder {} returned {} address(es) but none responded on port {}", seeder_host, addrs.len(), port)
}

/// Returns the block explorer base URL for the given network, or `None`
/// for networks without a known explorer.
pub fn explorer_base(network_id: &NetworkId) -> Option<String> {
    use kaspa_consensus_core::network::NetworkType;
    match network_id.network_type {
        NetworkType::Mainnet => Some("https://kaspa.stream".to_string()),
        NetworkType::Testnet => {
            let suffix = network_id.suffix.unwrap_or(10);
            Some(format!("https://tn{}.kaspa.stream", suffix))
        }
        _ => None,
    }
}

fn home_dir() -> Option<String> {
    std::env::var("HOME").ok().or_else(|| std::env::var("USERPROFILE").ok())
}
