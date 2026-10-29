use std::str::FromStr;

use ootle_rs::{provider::ProviderBuilder, template_types::constants::TARI_TOKEN, ToAccountAddress};
use tari_ootle_address::OotleAddress;

/// Revealed (vault) TARI balance in micro-TARI. Stealth UTXOs come from the scanner, not here.
pub async fn get_revealed_tari_balance(indexer_url: String, address: String) -> anyhow::Result<u64> {
    let address = OotleAddress::from_str(&address)?;
    let provider = ProviderBuilder::new().connect(&indexer_url).await?;
    let balance = provider.get_account_balance(address.to_account_address(), TARI_TOKEN).await?;
    balance.to_u64_checked().ok_or_else(|| anyhow::anyhow!("negative balance"))
}
