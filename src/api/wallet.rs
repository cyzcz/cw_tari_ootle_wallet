use std::str::FromStr;

use ootle_rs::Network;
use tari_ootle_address::OotleAddress;
use tari_common_types::seeds::{cipher_seed::CipherSeed, mnemonic::{Mnemonic, MnemonicLanguage}};

use crate::domain::keys::{account_keys, parse_seed};

pub enum TariOotleNetwork {
    MainNet,
    Esmeralda,
}

impl From<TariOotleNetwork> for Network {
    fn from(n: TariOotleNetwork) -> Self {
        match n {
            TariOotleNetwork::MainNet => Network::MainNet,
            TariOotleNetwork::Esmeralda => Network::Esmeralda,
        }
    }
}

pub struct WalletCreated {
    pub seed_words: String,
    pub address: String,
}

pub fn create_wallet(network: TariOotleNetwork) -> anyhow::Result<WalletCreated> {
    let seed = CipherSeed::random();
    let words = seed.to_mnemonic(MnemonicLanguage::English, None)?;
    Ok(WalletCreated {
        seed_words: words.join(" ").reveal().clone(),
        address: account_keys(&seed, network.into(), 0).to_address().to_string(),
    })
}

/// Address of account 0 for these seed words. Same words as the L1 (Minotari) wallet.
pub fn restore_wallet(seed_words: String, network: TariOotleNetwork) -> anyhow::Result<String> {
    let seed = parse_seed(&seed_words)?;
    Ok(account_keys(&seed, network.into(), 0).to_address().to_string())
}

pub fn validate_address(address: String) -> bool {
    OotleAddress::from_str(&address).is_ok_and(|a| a.validate().is_ok())
}
