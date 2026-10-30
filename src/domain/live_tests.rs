// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

//! Live Esmeralda checks. `cargo test --lib live -- --ignored --nocapture`
//! (OOTLE_INDEXER_URL overrides the indexer.)
//!
//! The send round trip needs a funded wallet: put its seed words in OOTLE_FUNDED_SEED (fund it
//! from Universe or a claim). The Esmeralda faucet hands out a fixed 1,000 TARI and was down to
//! 999.65 on 2026-10-29, so `faucet` fails until it is refilled.

use std::num::NonZeroU64;

use ootle_rs::{template_types::constants::TARI, Network};
use tari_common_types::seeds::cipher_seed::CipherSeed;

use super::{keys::AccountKeys, scan::scan, send, store::TxState, wallet::Wallet};

fn indexer() -> String {
    std::env::var("OOTLE_INDEXER_URL").unwrap_or_else(|_| "https://esme-indexer-a.ootle.tari.com/".into())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: needs the Esmeralda indexer"]
async fn live_fresh_wallet_scans_clean() {
    let dir = tempfile::tempdir().unwrap();
    // OOTLE_BIRTHDAY=0 times a restore-style scan from genesis.
    let epoch = match std::env::var("OOTLE_BIRTHDAY") {
        Ok(b) => b.parse().unwrap(),
        Err(_) => crate::api::wallet::get_current_epoch(indexer()).await.unwrap(),
    };
    let keys = AccountKeys::from_seed(&CipherSeed::random(), Network::Esmeralda, 0);
    let w = Wallet::open("live-fresh", dir.path().to_str().unwrap(), keys, indexer(), epoch).unwrap();
    let t = std::time::Instant::now();
    let mut passes = 0;
    scan(&w, |_| passes += 1).await.expect("scan");
    println!("fresh scan: {passes} passes in {:?}", t.elapsed());
    let (bal, cursor, vs) = w.read(|s| (s.balance(), s.cursor.len(), s.validator_set.is_some()));
    assert_eq!((bal.available, bal.locked), (0, 0));
    assert!(cursor > 0, "cursor not saved");
    assert!(vs, "validator set not saved");
    // A second scan resumes from the saved cursor.
    let t = std::time::Instant::now();
    scan(&w, |_| {}).await.expect("rescan");
    println!("resumed scan in {:?}", t.elapsed());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: needs OOTLE_FUNDED_SEED holding a few tTARI on Esmeralda"]
async fn live_send_round_trip() {
    let seed = std::env::var("OOTLE_FUNDED_SEED").expect("set OOTLE_FUNDED_SEED");
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().to_str().unwrap();
    let epoch = crate::api::wallet::get_current_epoch(indexer()).await.unwrap();
    // The funded wallet scans from genesis, like a restore.
    let a = Wallet::open("live-a", data, AccountKeys::from_words(&seed, Network::Esmeralda).unwrap(), indexer(), 0).unwrap();
    let b_keys = AccountKeys::from_seed(&CipherSeed::random(), Network::Esmeralda, 0);
    let b = Wallet::open("live-b", data, b_keys, indexer(), epoch).unwrap();

    let t = std::time::Instant::now();
    scan(&a, |more| println!("a pass, has_more={more}")).await.expect("scan a");
    println!("restore scan took {:?}", t.elapsed());
    let funded = a.read(|s| s.balance().available);
    println!("a available {funded}");
    assert!(funded > TARI, "OOTLE_FUNDED_SEED has no stealth TARI");

    let amount = NonZeroU64::new(TARI / 2).unwrap();
    let fee = send::estimate_fee(&a, &b.address, amount).await.expect("estimate");
    println!("fee {fee}");
    let tx = send::send(&a, &b.address, amount, fee, Some("cake live test".into()), |_| {})
        .await
        .expect("send");
    println!("send tx {tx}");

    scan(&a, |_| {}).await.expect("rescan a");
    scan(&b, |_| {}).await.expect("scan b");
    let (a_bal, a_txs) = a.read(|s| (s.balance(), s.transactions.clone()));
    let (b_bal, b_txs) = b.read(|s| (s.balance(), s.transactions.clone()));
    println!("a available {} locked {}; b available {}", a_bal.available, a_bal.locked, b_bal.available);
    assert_eq!(b_bal.available, TARI / 2);
    assert_eq!(b_txs.len(), 1);
    assert_eq!(b_txs[0].memo.as_deref(), Some("cake live test"));
    assert_eq!(a_bal.locked, 0);
    assert_eq!(a_bal.available, funded - TARI / 2 - fee);
    assert_eq!(a_txs.iter().find(|t| t.id == tx).unwrap().state, TxState::Completed);
    // Change comes back to `a` but is not listed as income.
    assert!(!a_txs.iter().any(|t| t.created_at_ms > a_txs.iter().find(|t| t.id == tx).unwrap().created_at_ms));
}
