use std::str::FromStr;

use ootle_rs::{crypto::derive_ristretto_key, keys::OotleSecretKey, Network};
use tari_common_types::seeds::{cipher_seed::CipherSeed, mnemonic::Mnemonic, seed_words::SeedWords};

// Same branch labels walletd's KeyManager uses (KeyBranch::Account / ViewOnlyKey), so a seed
// restored here lands on the same account as walletd and Universe.
const ACCOUNT_BRANCH: &[u8] = b"account";
const VIEW_BRANCH: &[u8] = b"view_only_key";

pub(crate) fn parse_seed(seed_words: &str) -> anyhow::Result<CipherSeed> {
    let words = SeedWords::from_str(seed_words).map_err(|e| anyhow::anyhow!("invalid seed words: {e}"))?;
    Ok(CipherSeed::from_mnemonic(&words, None)?)
}

pub(crate) fn account_keys(seed: &CipherSeed, network: Network, index: u64) -> OotleSecretKey {
    OotleSecretKey::new(
        network,
        derive_ristretto_key(seed.entropy(), ACCOUNT_BRANCH, index),
        derive_ristretto_key(seed.entropy(), VIEW_BRANCH, index),
    )
}
