use std::sync::Arc;

use anyhow::{Context, Result};
use kaspa_consensus_core::network::NetworkId;
use kaspa_wallet_core::wallet::Wallet;
use kaspa_wrpc_client::resolver::Resolver;

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
/// If `rpc_url` is `Some`, that URL is used directly; otherwise the public resolver is used.
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

fn home_dir() -> Option<String> {
    std::env::var("HOME").ok().or_else(|| std::env::var("USERPROFILE").ok())
}
