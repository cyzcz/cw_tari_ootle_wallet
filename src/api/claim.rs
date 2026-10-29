//! Claiming L1 (Minotari) burns into this wallet.

use crate::{
    api::send::SendEvent,
    domain::{
        burn_proof::BurnProof,
        send,
        store::{now_ms, ClaimRecord, ClaimState},
        wallet::Wallet,
    },
    frb_generated::StreamSink,
};

pub enum ClaimStatus {
    /// Waiting for the network to accept it (the burn must be a few epochs old), or the last attempt failed.
    Pending,
    Submitted,
    Claimed,
}

pub struct ClaimDto {
    /// The burn output's commitment, hex.
    pub id: String,
    pub amount: u64,
    pub status: ClaimStatus,
    pub tx_id: Option<String>,
    pub last_error: Option<String>,
    /// The L2 accepts the claim once its epoch is past this.
    pub mined_in_epoch: u64,
    pub created_at_ms: i64,
}

fn dto(c: &ClaimRecord) -> ClaimDto {
    ClaimDto {
        id: c.id.clone(),
        amount: c.value,
        status: match c.state {
            ClaimState::Pending => ClaimStatus::Pending,
            ClaimState::Submitted => ClaimStatus::Submitted,
            ClaimState::Claimed => ClaimStatus::Claimed,
        },
        tx_id: c.tx_id.clone(),
        last_error: c.last_error.clone(),
        mined_in_epoch: c.mined_in_epoch,
        created_at_ms: c.created_at_ms,
    }
}

/// Stores a burn proof from the L1 wallet (the `proof_json` of `cw_tari_wallet::get_burn_proofs`).
/// Adding the same proof twice is a no-op.
pub fn add_burn_proof(name: String, proof_json: String) -> anyhow::Result<ClaimDto> {
    let proof = BurnProof::from_json(&proof_json)?;
    let id = proof.claim_proof.commitment.to_string();
    Wallet::get(&name)?.write(|s| {
        if let Some(existing) = s.claims.iter().find(|c| c.id == id) {
            return dto(existing);
        }
        let record = ClaimRecord {
            id,
            value: proof.claim_proof.value,
            mined_in_epoch: proof.mined_in_epoch,
            proof_json,
            state: ClaimState::Pending,
            tx_id: None,
            last_error: None,
            created_at_ms: now_ms(),
        };
        let out = dto(&record);
        s.claims.push(record);
        out
    })
}

pub fn get_claims(name: String) -> anyhow::Result<Vec<ClaimDto>> {
    Ok(Wallet::get(&name)?.read(|s| s.claims.iter().map(dto).collect()))
}

/// Submits the claim and streams until it lands. A failure leaves the claim pending to retry.
pub async fn claim_burn(sink: StreamSink<SendEvent>, name: String, claim_id: String) -> anyhow::Result<()> {
    let wallet = Wallet::get(&name)?;
    let proof_json = wallet
        .read(|s| s.claims.iter().find(|c| c.id == claim_id).map(|c| c.proof_json.clone()))
        .ok_or_else(|| anyhow::anyhow!("no claim {claim_id}"))?;
    let proof = BurnProof::from_json(&proof_json)?;
    let set = |state: ClaimState, tx_id: Option<String>, error: Option<String>| {
        wallet.write(|s| {
            if let Some(c) = s.claims.iter_mut().find(|c| c.id == claim_id) {
                c.state = state;
                c.tx_id = tx_id.or(c.tx_id.take());
                c.last_error = error;
            }
        })
    };
    let result = send::claim(&wallet, proof, |p| match p {
        send::Progress::Submitted { tx_id } => {
            let _ = set(ClaimState::Submitted, Some(tx_id.clone()), None);
            let _ = sink.add(SendEvent::Submitted { tx_id });
        },
    })
    .await;
    match result {
        Ok(tx_id) => {
            set(ClaimState::Claimed, Some(tx_id.clone()), None)?;
            let _ = sink.add(SendEvent::Completed { tx_id });
        },
        Err(e) => {
            let message = format!("{e:#}");
            set(ClaimState::Pending, None, Some(message.clone()))?;
            let _ = sink.add(SendEvent::Failed { message });
        },
    }
    Ok(())
}
