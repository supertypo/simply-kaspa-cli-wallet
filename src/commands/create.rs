use std::sync::Arc;

use anyhow::{Context, Result};
use kaspa_bip32::{Language, Mnemonic, WordCount};
use kaspa_consensus_core::network::NetworkId;
use kaspa_wallet_core::{
    api::{
        message::{AccountsCreateRequest, PrvKeyDataCreateRequest, WalletCreateRequest},
        traits::WalletApi,
    },
    encryption::EncryptionKind,
    storage::keydata::PrvKeyDataVariantKind,
    wallet::args::{AccountCreateArgs, PrvKeyDataCreateArgs, WalletCreateArgs},
};
use kaspa_wallet_keys::secret::Secret;

use crate::wallet::{build_wallet, init_storage};

pub async fn run(
    network_id: NetworkId,
    rpc_url: Option<String>,
    wallet_name: String,
    account_name: Option<String>,
    import_mnemonic: Option<String>,
    password: String,
    payment_secret: Option<String>,
) -> Result<()> {
    init_storage(&network_id)?;

    // For wallet creation we do not need an RPC connection.
    let wallet = build_wallet(rpc_url, network_id)?;
    let wallet_arc = Arc::clone(&wallet);

    wallet.start().await.context("Failed to start wallet")?;

    let wallet_secret = Secret::new(password.into_bytes());

    // --- 1. Create wallet file ---
    let wallet_args = WalletCreateArgs::new(
        None,
        Some(wallet_name.clone()),
        EncryptionKind::XChaCha20Poly1305,
        None,
        false,
    );
    wallet_arc
        .clone()
        .wallet_create_call(WalletCreateRequest {
            wallet_secret: wallet_secret.clone(),
            wallet_args,
        })
        .await
        .context("Failed to create wallet")?;

    // --- 2. Mnemonic ---
    let (mnemonic_phrase, generated) = if let Some(phrase) = import_mnemonic {
        (phrase, false)
    } else {
        let mnemonic = Mnemonic::random(WordCount::Words24, Language::English)
            .context("Failed to generate mnemonic")?;
        (mnemonic.phrase().to_owned(), true)
    };

    // --- 3. Register private key ---
    let payment_secret_opt = payment_secret.as_deref().map(|s| Secret::new(s.as_bytes().to_vec()));
    let prv_key_args = PrvKeyDataCreateArgs::new(
        None,
        payment_secret_opt.clone(),
        Secret::new(mnemonic_phrase.as_bytes().to_vec()),
        PrvKeyDataVariantKind::Mnemonic,
    );
    let prv_key_resp = wallet_arc
        .clone()
        .prv_key_data_create_call(PrvKeyDataCreateRequest {
            wallet_secret: wallet_secret.clone(),
            prv_key_data_args: prv_key_args,
        })
        .await
        .context("Failed to register private key")?;

    // --- 4. Create account ---
    let account_create_args = AccountCreateArgs::new_bip32(
        prv_key_resp.prv_key_data_id,
        payment_secret_opt,
        account_name.clone().or_else(|| Some("default".to_string())),
        None,
    );
    let account_resp = wallet_arc
        .clone()
        .accounts_create_call(AccountsCreateRequest {
            wallet_secret,
            account_create_args,
        })
        .await
        .context("Failed to create account")?;

    wallet.stop().await.context("Failed to stop wallet")?;

    // --- Output ---
    if generated {
        println!();
        println!("NEW WALLET MNEMONIC (write this down and keep it safe!):");
        println!();
        println!("  {}", mnemonic_phrase);
        println!();
    }

    println!("Wallet     : {}", wallet_name);
    println!(
        "Account    : {}",
        account_name.as_deref().unwrap_or("default")
    );
    if let Some(addr) = &account_resp.account_descriptor.receive_address {
        println!("Address    : {}", addr);
    }
    println!("Network    : {}", network_id);

    Ok(())
}
