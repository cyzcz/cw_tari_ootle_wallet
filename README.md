# cw_tari_ootle_wallet

The Rust side of Tari L2 (Ootle) support in [Cake Wallet](https://github.com/tari-project/cake_wallet/tree/ootle_integration), exposed to Dart with [flutter_rust_bridge](https://github.com/fzyzcjy/flutter_rust_bridge) 2.11.1. It's the L2 sibling of [cw_tari_wallet](https://github.com/tari-project/cw_tari_wallet), which does the same job for L1 (Minotari).

It's a light wallet. It talks to a Tari indexer over HTTPS through [ootle-rs](https://crates.io/crates/ootle-rs), keeps a small JSON state file per wallet, and never runs a node or a wallet daemon. Keys come from the same 24 seed words as the L1 wallet, derived the way `tari_ootle_walletd` and Tari Universe derive them, so a seed restored here lands on the same account.

## What it does

- Create and restore wallets, derive the `otl_` address, validate addresses
- Scan for stealth TARI addressed to the wallet, track spends, and verify the indexer's answers against the network's validator set (`IndexerTrust::Verify`)
- Balance and history
- Estimate fees with dry runs and send stealth TARI
- Claim L1 burns, using the proof file `cw_tari_wallet`'s `get_burn_proofs` writes
- Take test funds from the Esmeralda faucet

The public surface is `src/api/*.rs`. Everything under `src/domain` is internal.

## Versions

The `ootle-rs` and `tari_ootle_*` crates are pinned to what Ootle v0.47.0 published, because that's what Esmeralda runs. v0.48 changed the transaction encoding and the Esmeralda indexers reject it. Bump them together when Esmeralda moves.

There is no mainnet indexer yet, so this is Esmeralda only for now.

## Building

```sh
cargo test --lib                       # unit tests
flutter_rust_bridge_codegen generate   # regenerate src/frb_generated.rs and .dart/
cargo ndk -t arm64-v8a --platform 21 build --release
scripts/file_license_check.sh          # every source file carries the Tari header
```

Cake's `scripts/prepare_tari_ootle.sh` clones a pinned rev of this repo, runs the codegen and builds the Android and iOS libraries.

On Android the app must call `CwTariOotlePlugin.initTls(context)` once after loading the library (see `src/android.rs`). The HTTPS client verifies certificates with `rustls-platform-verifier`, which needs the app Context over JNI and panics without it. That call fails under an x86_64 emulator's ARM translation, so test the x86_64 build on an emulator.

## Live tests

These run against the public Esmeralda indexer and are ignored by default.

```sh
cargo test --lib live_fresh -- --ignored --nocapture                            # a new wallet scans clean
OOTLE_FUNDED_SEED="word1 ... word24" cargo test --lib live_send -- --ignored    # send round trip
```

The send test needs a seed holding a few tTARI. The Esmeralda faucet hands out a fixed 1,000 TARI and was down to 999.65 on 2026-10-29, so it can't fund one until it's refilled.

## License

BSD-3-Clause. `src/domain/burn_proof.rs` ports `claim_proof_from_l1` from tari-ootle's `tari_ootle_app_utilities`, which is under the same license.
