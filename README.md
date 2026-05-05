# simply-kaspa-cli-wallet

A minimal, single-binary Kaspa CLI wallet designed for automation and scripting.

> **⚠ TESTNET ONLY**
> This wallet has not been properly vetted for mainnet use. Use on mainnet at your own risk.

Built on top of the [rusty-kaspa](https://github.com/kaspanet/rusty-kaspa/) SDK — key derivation, encrypted storage, UTXO management, and transaction construction are all handled by the SDK.

---

## Installation

**Requirements:** Rust 1.88+ (see `rust-toolchain.toml`)

```bash
# Build a release binary
cargo build --release

# The binary is at:
./target/release/kaspa-wallet
```

Or run directly without installing:

```bash
cargo run -- [OPTIONS] <COMMAND>
```

### Docker

```bash
cd docker && ./build.sh nopush local
docker run --rm supertypo/simply-kaspa-cli-wallet:local --help
```

---

## Wallet storage

Wallets are stored locally at:

```
~/.simply-kaspa-cli-wallet/<network>/<wallet_name>
```

The default wallet name is `main`. Use `--wallet-name` to work with multiple wallets side by side.

---

## Common commands

### Create a wallet

```bash
# Create a new wallet (generates a 24-word mnemonic)
kaspa-wallet create

# Import an existing mnemonic
kaspa-wallet create --import "word1 word2 ... word24"

# Create on testnet-10
kaspa-wallet --network testnet-10 create
```

You will be prompted for a wallet password (required) and optionally a BIP39 payment password.

### Check balance

```bash
kaspa-wallet balance

# Testnet-10
kaspa-wallet --network testnet-10 balance

# Named wallet
kaspa-wallet --wallet-name mywallet balance
```

### Send KAS

```bash
kaspa-wallet send --to-address kaspa:qr... --amount 10.5

# With a priority fee and a text memo
kaspa-wallet send --to-address kaspa:qr... --amount 10.5 --priority-fee 0.1 --payload "hello"

# Non-interactive (password via flag, skips confirmation prompt)
kaspa-wallet --password mypassword --no-confirmation send --to-address kaspa:qr... --amount 10.5
```

Before sending, an estimated fee and UTXO summary is shown. In interactive mode you confirm with `y`; in non-interactive mode a 5-second countdown is displayed. Pass `--no-confirmation` to skip both.

### Sweep UTXOs

Consolidates all UTXOs into a single UTXO — useful for wallets with many small inputs.

```bash
kaspa-wallet sweep
```

### Export mnemonic

```bash
kaspa-wallet export
```

Prints the 24-word seed phrase. Keep it safe.

---

## Global options

| Flag | Short | Description |
|---|---|---|
| `--network` | `-n` | Network: `mainnet`, `testnet-10`, `testnet-11`, `simnet`, `devnet` (default: `mainnet`) |
| `--wallet-name` | `-w` | Wallet filename under the storage directory (default: `main`) |
| `--password` | `-p` | Wallet password (omit to be prompted) |
| `--payment-secret` | | BIP39 passphrase (omit to be prompted during `create`/`export`) |
| `--rpc-url` | `-s` | wRPC node URL, e.g. `wrpc://127.0.0.1:17110` (omit to use the public resolver) |
| `--no-confirmation` | | Skip the confirmation prompt or countdown before `send`/`sweep` |

---

## Full help

```bash
kaspa-wallet --help
kaspa-wallet send --help
```
