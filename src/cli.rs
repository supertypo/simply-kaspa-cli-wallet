use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "kaspa-wallet", version, about = "Kaspa CLI wallet")]
pub struct Cli {
    /// wRPC server URL (e.g. wrpc://127.0.0.1:17110). Omit to use the public resolver.
    #[arg(long, short = 's', global = true)]
    pub rpc_url: Option<String>,

    /// Network ID (mainnet | testnet-10 | testnet-11 | simnet | devnet). [default: mainnet]
    #[arg(long, short = 'n', global = true, default_value = "mainnet")]
    pub network: String,

    /// Wallet password (omit to be prompted interactively)
    #[arg(long, short = 'p', global = true)]
    pub password: Option<String>,

    /// Wallet name (filename under ~/.simply-kaspa-cli-wallet/<network>/). [default: main]
    #[arg(long, short = 'w', global = true, default_value = "main")]
    pub wallet_name: String,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Create a new wallet (and its first account)
    Create {
        /// Account name (optional, defaults to "default")
        #[arg(long)]
        account_name: Option<String>,

        /// Import an existing mnemonic phrase (24 words). Omit to generate a new one.
        #[arg(long, value_name = "MNEMONIC")]
        import: Option<String>,
    },

    /// Show the balance of a wallet
    Balance,

    /// Sweep all UTXOs into a single UTXO at the account's first address
    Sweep,
}
