# simply-kaspa-cli-wallet — Agent Reference

Important: Please keep this file up to date.

## What This Project Does

A minimal, single-binary Kaspa CLI wallet (`kaspa-wallet`) built to enable automation.
It exposes four subcommands — `create`, `balance`, `send`, `sweep` — via a thin Rust wrapper around the rusty-kaspa wallet SDK. 
Wallet files are stored locally in `~/.simply-kaspa-cli-wallet/<network>/<wallet_name>`.

---

## What Is Inherited from rusty-kaspa

Everything listed below is provided by the SDK crates and requires **no custom logic** in this repo:

| Concern | SDK crate |
|---|---|
| BIP32 HD key derivation & 24-word mnemonic generation/import | `kaspa-bip32` |
| Encrypted local wallet storage (XChaCha20Poly1305) | `kaspa-wallet-core` (local store) |
| UTXO scanning, tracking, and Balance event emission | `kaspa-wallet-core` (event multiplexer) |
| Multi-TX transaction generation (batching when UTXOs exceed one tx) | `kaspa-wallet-core` `account::send` / `account::sweep` |
| Fee calculation | SDK transaction generator |
| wRPC node connection and all RPC calls | `kaspa-wrpc-client` + `kaspa-rpc-core` |
| Public node resolution for mainnet / testnets | `kaspa-wrpc-client::resolver::Resolver` |
| Address derivation and the deterministic account model | `kaspa-wallet-core` |
| sompi ↔ KAS string conversion | `kaspa-wallet-core::utils` |

---

## What This Repo Adds

- **CLI parsing** (clap) — global flags: `--rpc-url`, `--network`, `--password`, `--wallet-name`.
- **DNS-seeder resolution for testnet-10 and testnet-12**: queries `n-testnet-10.kaspa.ws` / `n-testnet-12.kaspa.ws` respectively, TCP-probes each returned IPv4 (3 s timeout), returns the first live node as `ws://ip:port`. All other networks (including mainnet) fall through to the SDK's public resolver.
- **Storage path wiring**: calls the SDK's unsafe `set_default_storage_folder` to point at `~/.simply-kaspa-cli-wallet/<network>/` before any wallet operation.
- **Per-address UTXO breakdown** in `balance`: combines the SDK's mature-UTXO data with a direct `rpc_api().get_utxos_by_addresses()` call to also surface pending (immature coinbase) amounts.
- **Explorer links**: prints `kaspa.stream` (mainnet) or `tn{N}.kaspa.stream` (testnet) URLs for each submitted transaction.
- **Password prompting** via `rpassword` (with confirmation on `create`).
- **Docker build** (Alpine, musl, static-ish binary, non-root user 13337).

---

## Architecture

```
src/
  main.rs          — arg parsing, password prompting, dispatch
  cli.rs           — clap structs (Cli, Command)
  wallet.rs        — init_storage, build_wallet, resolve_url, explorer_base
  commands/
    create.rs      — local-only; no RPC needed
    balance.rs     — connects, waits for Balance events, queries node for pending
    send.rs        — connects, waits for Balance events, calls account.send()
    sweep.rs       — connects, waits for Balance events, calls account.sweep()
```

### Non-obvious implementation details

- **`create` never connects to a node.** Wallet and account creation is purely a local key/file operation.
- **Event subscription must happen before `wallet.start()`** to avoid missing Balance events. All commands that need RPC subscribe first, then call `start()` + `connect()`.
- **UTXO readiness pattern**: after `activate_accounts`, the code waits (up to 30 s) for a `Balance` event per account before proceeding. This is how the SDK signals that the initial UTXO scan has been submitted.
- **`set_default_storage_folder` is `unsafe`** and must be called once, before wallet init, from the main thread. It is wrapped in `wallet::init_storage`.
- **Units**: user-facing amounts are in KAS; all SDK internals use sompi (1 KAS = 100 000 000 sompi). Conversion uses `try_kaspa_str_to_sompi` / `sompi_to_kaspa_string_with_suffix`.
- **Only the first active account is used** for `send` and `sweep` (`active_accounts().first()`).
- **Sweep** consolidates all UTXOs into one. Useful for wallets with many small UTXOs (reduces future fees). The notifier prints a running "UTXOs remaining" counter.

---

## Build & Run

```bash
# development
cargo run -- --network testnet-12 balance

# release
cargo build --release

# Docker
docker build -f docker/Dockerfile -t kaspa-wallet .
```
