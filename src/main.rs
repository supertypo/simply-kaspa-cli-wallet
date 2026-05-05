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
            let (password, payment_secret) = match args.password {
                Some(p) => {
                    // Non-interactive: use --payment-secret if provided, else None
                    (p, args.payment_secret)
                }
                None => {
                    // Interactive: prompt for wallet password, then optionally payment password
                    let pwd = read_password_confirmed("Wallet password: ", "Confirm password: ")?;
                    let ps = if args.payment_secret.is_some() {
                        args.payment_secret
                    } else {
                        ask_payment_secret()?
                    };
                    (pwd, ps)
                }
            };
            commands::create::run(
                network_id,
                args.rpc_url,
                args.wallet_name,
                account_name,
                import,
                password,
                payment_secret,
            )
            .await?;
        }
        Command::Balance => {
            let password = match args.password {
                Some(p) => p,
                None => read_password("Wallet password: ")?,
            };
            commands::balance::run(
                network_id,
                args.rpc_url,
                args.wallet_name,
                password,
            )
            .await?;
        }
        Command::Send { to_address, amount, priority_fee, payload } => {
            let (password, interactive, priority_fee) = match args.password {
                Some(p) => (p, false, priority_fee),
                None => {
                    let pwd = read_password("Wallet password: ")?;
                    let pf = if priority_fee.is_some() {
                        priority_fee
                    } else {
                        ask_priority_fee()?
                    };
                    (pwd, true, pf)
                }
            };
            commands::send::run(
                network_id,
                args.rpc_url,
                args.wallet_name,
                password,
                to_address,
                amount,
                priority_fee,
                payload,
                interactive,
                args.no_confirmation,
            )
            .await?;
        }
        Command::Sweep => {
            let (password, interactive) = match args.password {
                Some(p) => (p, false),
                None => (read_password("Wallet password: ")?, true),
            };
            commands::sweep::run(
                network_id,
                args.rpc_url,
                args.wallet_name,
                password,
                interactive,
                args.no_confirmation,
            )
            .await?;
        }
        Command::Export => {
            let password = match args.password {
                Some(p) => p,
                None => read_password("Wallet password: ")?,
            };
            commands::export::run(
                network_id,
                args.wallet_name,
                password,
                args.payment_secret,
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

/// Interactively ask whether the user wants a BIP39 payment password.
/// Returns `Some(secret)` if yes, `None` if no.
fn ask_payment_secret() -> Result<Option<String>> {
    use std::io::{Write, BufRead};
    print!("Use a payment password? [y/N]: ");
    std::io::stdout().flush().context("Failed to flush stdout")?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).context("Failed to read input")?;
    if line.trim().eq_ignore_ascii_case("y") {
        let ps = read_password_confirmed("Payment password: ", "Confirm payment password: ")?;
        Ok(Some(ps))
    } else {
        Ok(None)
    }
}

/// Interactively prompt for an optional priority fee.
/// Returns `None` if the user presses Enter (meaning 0 / no extra fee).
fn ask_priority_fee() -> Result<Option<String>> {
    use std::io::{Write, BufRead};
    print!("Priority fee in KAS [0]: ");
    std::io::stdout().flush().context("Failed to flush stdout")?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).context("Failed to read input")?;
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed == "0" {
        Ok(None)
    } else {
        Ok(Some(trimmed.to_string()))
    }
}
