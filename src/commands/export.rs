use std::sync::Arc;

use anyhow::{Context, Result};
use kaspa_consensus_core::network::NetworkId;
use kaspa_wallet_core::{
    api::{
        message::{AccountsEnumerateRequest, PrvKeyDataGetRequest, WalletOpenRequest},
        traits::WalletApi,
    },
};
use kaspa_wallet_keys::secret::Secret;

use crate::wallet::{build_wallet, init_storage};

pub async fn run(
    network_id: NetworkId,
    wallet_name: String,
    password: String,
    payment_secret: Option<String>,
) -> Result<()> {
    init_storage(&network_id)?;

    // Export is local-only; no RPC connection needed.
    let wallet = build_wallet(None, network_id)?;
    let wallet_arc = Arc::clone(&wallet);

    wallet.start().await.context("Failed to start wallet")?;

    let wallet_secret = Secret::new(password.into_bytes());

    // Open wallet to load account descriptors
    wallet_arc
        .clone()
        .wallet_open_call(WalletOpenRequest {
            wallet_secret: wallet_secret.clone(),
            filename: Some(wallet_name.clone()),
            account_descriptors: true,
            legacy_accounts: None,
        })
        .await
        .context("Failed to open wallet")?;

    // Enumerate accounts to get full descriptors (including prv_key_data_ids)
    let descriptors = wallet_arc
        .clone()
        .accounts_enumerate_call(AccountsEnumerateRequest {})
        .await
        .context("Failed to enumerate accounts")?
        .account_descriptors;

    let descriptor = descriptors.first().context("No accounts found in wallet")?;

    let prv_key_data_id = descriptor
        .prv_key_data_ids()
        .into_iter()
        .next()
        .context("Account has no associated private key data (watch-only?)")?;

    // Fetch key data — decrypted with wallet_secret; payload may still require payment_secret.
    let prv_key_data = wallet_arc
        .clone()
        .prv_key_data_get_call(PrvKeyDataGetRequest {
            wallet_secret: wallet_secret.clone(),
            prv_key_data_id,
        })
        .await
        .context("Failed to retrieve private key data")?
        .prv_key_data
        .context("Private key data not found")?;

    wallet.stop().await.ok();

    // Resolve payment secret: flag → interactive prompt → None
    let payment_secret = if prv_key_data.payload.is_encrypted() {
        let ps = match payment_secret {
            Some(s) => s,
            None => rpassword::prompt_password("Payment password: ")
                .context("Failed to read payment password")?,
        };
        Some(Secret::new(ps.into_bytes()))
    } else {
        None
    };

    let mnemonic = prv_key_data
        .as_mnemonic(payment_secret.as_ref())
        .context("Failed to decrypt mnemonic")?
        .context("This account type does not have a mnemonic (keypair or watch-only?)")?;

    println!();
    println!("WALLET MNEMONIC (keep this secret!):");
    println!();
    println!("  {}", mnemonic.phrase());
    println!();
    println!("Wallet : {}", wallet_name);
    println!("Network: {}", network_id);

    Ok(())
}
