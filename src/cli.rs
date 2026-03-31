use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "kaspa-wallet", version, about = "Kaspa CLI wallet")]
pub struct Cli {
    /// wRPC server URL (e.g. wrpc://127.0.0.1:17110). Omit to use the public resolver.
    #[arg(short = 's', long = "rpc-url", global = true)]
    pub rpc_url: Option<String>,

    /// Network ID (mainnet | testnet-10 | testnet-11 | simnet | devnet). [default: mainnet]
    #[arg(short = 'n', long = "network", global = true, default_value = "mainnet")]
    pub network: String,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Create a new wallet (and its first account)
    Create {
        /// Wallet name (used as filename under ~/.simply-kaspa-cli-wallet/<network>/)
        #[arg(long, default_value = "main")]
        wallet_name: String,

        /// Account name (optional, defaults to "default")
        #[arg(long)]
        account_name: Option<String>,

        /// Import an existing mnemonic phrase (24 words). Omit to generate a new one.
        #[arg(long, value_name = "MNEMONIC")]
        import: Option<String>,
    },

    /// Show the balance of a wallet
    Balance {
        /// Wallet name
        #[arg(long, default_value = "main")]
        wallet_name: String,

        /// Show per-account breakdown
        #[arg(short = 'v', long)]
        verbose: bool,
    },
}
