mod cli;
mod commands;
mod wallet;

use anyhow::{Context, Result};
use clap::Parser;
use cli::{Cli, Command};
use kaspa_consensus_core::network::NetworkId;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();

    let network_id: NetworkId = args.network.parse().context("Invalid --network value")?;

    match args.command {
        Command::Create { wallet_name, account_name, import } => {
            let password = read_password("Wallet password: ")?;
            commands::create::run(
                network_id,
                args.rpc_url,
                wallet_name,
                account_name,
                import,
                password,
            )
            .await?;
        }
        Command::Balance { wallet_name, verbose } => {
            let password = read_password("Wallet password: ")?;
            commands::balance::run(
                network_id,
                args.rpc_url,
                wallet_name,
                verbose,
                password,
            )
            .await?;
        }
    }

    Ok(())
}

fn read_password(prompt: &str) -> Result<String> {
    rpassword::prompt_password(prompt).context("Failed to read password")
}
