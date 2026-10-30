// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

//! One scan: refresh the revealed balance, then drain the stealth UTXO stream for TARI from the
//! wallet's cursor, keeping the outputs addressed to this account and marking the ones it spent.
//!
//! Mirrors walletd's `UtxoScannerRound` as of Ootle v0.47.
//!
//! ponytail: v0.47's stream is served unproven and its cursor is not bounded by verified
//! checkpoints; only the outputs we keep are verified (get_substate). Ootle v0.48 adds checkpoint
//! bounds and `spends_incomplete`; adopt them when Esmeralda moves to v0.48.

use std::collections::HashSet;

use futures::StreamExt;
use log::{debug, warn};
use ootle_byte_type::FromByteType;
use ootle_rs::{
    crypto::{encrypted_data::unblind_output, kdfs, StealthCryptoApi},
    provider::{ShardCursor, StealthUtxoFrame, StealthUtxoWatchRequest},
    template_types::{constants::TARI_TOKEN, crypto::RistrettoPublicKeyBytes, UtxoAddress, UtxoId},
    Epoch, ToAccountAddress,
};
use tari_crypto::tari_utilities::ByteArray;
use tari_crypto::{keys::PublicKey, ristretto::RistrettoPublicKey};
use tari_ootle_common_types::engine_types::{substate::SubstateId, Utxo};

use super::{
    store::{now_ms, OutputStatus, OwnedOutput, TxKind, TxRecord, TxState},
    wallet::{Provider, Wallet},
};

const PER_SHARD_LIMIT: u32 = 1000;

/// Runs passes until the indexer has nothing more for this wallet. `on_pass` fires after each
/// persisted pass, so the UI can refresh as the backlog drains.
pub(crate) async fn scan(wallet: &Wallet, mut on_pass: impl FnMut(bool)) -> anyhow::Result<()> {
    let provider = wallet.provider().await?;
    // Walk the signed epoch handovers from the set we trust, so verified reads below have an anchor.
    provider.sync_validator_set().await?;
    refresh_revealed(wallet, &provider).await?;
    loop {
        let has_more = pass(wallet, &provider).await?;
        on_pass(has_more);
        if !has_more {
            break;
        }
    }
    expire_stale_sends(wallet, &provider).await?;
    wallet.remember_validator_set(&provider).await
}

async fn refresh_revealed(wallet: &Wallet, provider: &Provider) -> anyhow::Result<()> {
    let component = wallet.address.to_account_address();
    // A fresh account has no component until something is deposited into it.
    let revealed = if provider.get_substate(component).await?.is_some() {
        provider
            .get_account_balance(component, TARI_TOKEN)
            .await?
            .to_u64_checked()
            .unwrap_or(0)
    } else {
        0
    };
    wallet.write(|s| s.revealed = revealed)
}

async fn pass(wallet: &Wallet, provider: &Provider) -> anyhow::Result<bool> {
    let mut cursor = match wallet.read(|s| s.cursor.clone()) {
        c if c.is_empty() => ShardCursor::genesis(provider.get_num_preshards().await?),
        c => ShardCursor::from_pairs(c),
    };
    let (birthday, has_live) = wallet.read(|s| (s.birthday_epoch, s.has_live_outputs()));
    let request = StealthUtxoWatchRequest {
        resource_address: TARI_TOKEN,
        from_epoch: Epoch(birthday),
        shard_state_versions: cursor.to_pairs(),
        // Spends only matter once we hold something that can be spent.
        unspent_only: !has_live,
        per_shard_limit: PER_SHARD_LIMIT,
    };

    let crypto = StealthCryptoApi;
    let mut candidates = Vec::new();
    let mut spent = Vec::new();
    let mut has_more = false;
    let mut stream = Box::pin(provider.watch_stealth_utxos(request).into_stream());
    while let Some(frame) = stream.next().await {
        match frame? {
            StealthUtxoFrame::StartOfShard { has_more: more, .. } => has_more |= more,
            StealthUtxoFrame::Unspent { tag, public_nonce } => {
                let Ok(nonce) = public_nonce.try_from_byte_type() else {
                    continue;
                };
                let expected = crypto.derive_stealth_output_tag(
                    wallet.keys.network,
                    &wallet.keys.view_secret,
                    &nonce,
                    &TARI_TOKEN,
                );
                if expected == tag {
                    candidates.push((tag, public_nonce));
                }
            }
            StealthUtxoFrame::Spent { id, .. } | StealthUtxoFrame::Burnt { id, .. } => {
                spent.push(id)
            }
            StealthUtxoFrame::EndOfShard {
                shard,
                max_state_version,
            } => cursor.observe(shard, max_state_version),
        }
    }

    let found = recover(wallet, provider, &candidates).await?;

    wallet.write(|s| {
        for (id, output) in found {
            if s.outputs.contains_key(&id) {
                continue;
            }
            // Change and claim/faucet outputs come from our own transactions; only the rest is income.
            if !s.own_outputs.remove(&id) {
                s.transactions.push(TxRecord {
                    id: id.clone(),
                    kind: TxKind::Received,
                    amount: output.value,
                    fee: 0,
                    state: TxState::Completed,
                    counterparty: None,
                    memo: output.memo.clone(),
                    error: None,
                    max_epoch: None,
                    created_at_ms: output.found_at_ms,
                });
            }
            s.outputs.insert(id, output);
        }
        let spent: HashSet<String> = spent.iter().map(UtxoId::to_string).collect();
        let mut settled = HashSet::new();
        for (_, o) in s.outputs.iter_mut().filter(|(id, _)| spent.contains(*id)) {
            if let Some(tx) = o.locked_by.take() {
                settled.insert(tx);
            }
            o.status = OutputStatus::Spent;
        }
        // An input of a send we lost track of (app killed mid-watch) was spent, so the send landed.
        for tx in settled {
            if let Some(t) = s.tx_mut(&tx) {
                if t.state == TxState::Pending {
                    t.state = TxState::Completed;
                }
            }
        }
        s.cursor = cursor.to_pairs();
    })?;
    Ok(has_more)
}

/// Fetch the matched outputs, re-read each through the verifying provider, and keep the ones this
/// account can actually spend.
async fn recover(
    wallet: &Wallet,
    provider: &Provider,
    candidates: &[(
        ootle_rs::template_types::crypto::UtxoTag,
        RistrettoPublicKeyBytes,
    )],
) -> anyhow::Result<Vec<(String, OwnedOutput)>> {
    let mut found = Vec::new();
    for (id, _) in provider.fetch_unspent_utxos(TARI_TOKEN, candidates).await? {
        if wallet.read(|s| s.outputs.contains_key(&id.to_string())) {
            continue;
        }
        // fetch_unspent_utxos is served unproven; get_substate verifies under IndexerTrust::Verify.
        let substate_id = SubstateId::from(UtxoAddress::new(TARI_TOKEN, id));
        let Some(substate) = provider.get_substate(substate_id).await? else {
            continue;
        };
        let Some(utxo) = substate.into_substate_value().into_utxo() else {
            continue;
        };
        if let Some(output) = decrypt_if_ours(wallet, id, &utxo) {
            found.push((id.to_string(), output));
        }
    }
    Ok(found)
}

/// The receive half of `ootle_sdk_core::stealth::scan_stealth_output`: decrypt with the view key,
/// then check the tag and the one-time spend key `H(k·R)·G + K`, since a sender can encrypt to our
/// view key while committing a spend key it controls.
fn decrypt_if_ours(wallet: &Wallet, id: UtxoId, utxo: &Utxo) -> Option<OwnedOutput> {
    let out = utxo.output()?;
    if utxo.is_frozen() {
        warn!("Stealth output {id} is frozen; skipping");
        return None;
    }
    let nonce: RistrettoPublicKey = out.output.public_nonce.try_from_byte_type().ok()?;
    let keys = &wallet.keys;
    let key = kdfs::encrypted_data_dh_kdf_aead(&keys.view_secret, &nonce);
    let decrypted = unblind_output(
        &id.into_commitment_bytes(),
        &out.output.encrypted_data,
        &key,
        false,
    )
    .ok()?;

    let crypto = StealthCryptoApi;
    if crypto.derive_stealth_output_tag(keys.network, &keys.view_secret, &nonce, &TARI_TOKEN)
        != out.tag
    {
        return None;
    }
    // Script-locked outputs are not spendable by the account key alone; v1 ignores them.
    let spend_key = out.auth.spend_key()?;
    let owner_secret =
        crypto.derive_stealth_owner_secret(keys.network, &keys.account_secret, &nonce);
    if RistrettoPublicKey::from_secret_key(&owner_secret).as_bytes() != spend_key.as_bytes() {
        debug!("Stealth output {id} decrypts with our view key but its spend key is not ours");
        return None;
    }
    Some(OwnedOutput {
        value: decrypted.value(),
        status: OutputStatus::Unspent,
        locked_by: None,
        memo: decrypted
            .memo()
            .and_then(|m| m.as_memo_message())
            .map(str::to_string),
        found_at_ms: now_ms(),
    })
}

/// A send still pending after its last valid epoch never landed if its inputs are still unspent:
/// release them.
async fn expire_stale_sends(wallet: &Wallet, provider: &Provider) -> anyhow::Result<()> {
    let epoch = provider.get_epoch().await?.as_u64();
    wallet.write(|s| {
        let stale: Vec<String> = s
            .transactions
            .iter()
            .filter(|t| t.state == TxState::Pending && t.max_epoch.is_some_and(|m| epoch > m))
            .map(|t| t.id.clone())
            .collect();
        for id in stale {
            s.settle(
                &id,
                false,
                0,
                Some("expired before it was finalized".into()),
            );
        }
    })
}
