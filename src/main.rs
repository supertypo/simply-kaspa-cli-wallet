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
        Command::Create { account_name, import } => {
            let password = match args.password {
                Some(p) => p,
                None => read_password_confirmed("Wallet password: ", "Confirm password: ")?,
            };
            commands::create::run(
                network_id,
                args.rpc_url,
                args.wallet_name,
                account_name,
                import,
                password,
            )
            .await?;
        }
        Command::Balance => {
            let password = match args.password {
                Some(p) => p,
                None => read_password("Wallet password: ")?,
            };
            let timeout = std::time::Duration::from_secs(args.timeout);
            commands::balance::run(
                network_id,
                args.rpc_url,
                args.wallet_name,
                password,
                timeout,
            )
            .await?;
        }
    }

    Ok(())
}

fn read_password(prompt: &str) -> Result<String> {
    rpassword::prompt_password(prompt).context("Failed to read password")
}

fn read_password_confirmed(prompt: &str, confirm_prompt: &str) -> Result<String> {
    let password = rpassword::prompt_password(prompt).context("Failed to read password")?;
    let confirm = rpassword::prompt_password(confirm_prompt).context("Failed to read password")?;
    anyhow::ensure!(password == confirm, "Passwords do not match");
    Ok(password)
}
