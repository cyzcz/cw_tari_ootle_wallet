// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

//! Open wallets, keyed by name, and the indexer connection for each.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, LazyLock, Mutex, MutexGuard, RwLock},
};

use anyhow::anyhow;
use ootle_rs::{
    key_provider::PrivateKeyProvider,
    provider::{IndexerProvider, IndexerTrust, ProviderBuilder},
    wallet::OotleWallet,
};
use tari_ootle_address::OotleAddress;

use super::{keys::AccountKeys, store::Store};

pub(crate) type Provider = IndexerProvider<OotleWallet>;

pub(crate) struct Wallet {
    pub keys: AccountKeys,
    pub address: OotleAddress,
    store_path: PathBuf,
    store: Mutex<Store>,
    indexer_url: RwLock<String>,
}

static WALLETS: LazyLock<Mutex<HashMap<String, Arc<Wallet>>>> = LazyLock::new(Default::default);

impl Wallet {
    pub fn open(
        name: &str,
        data_dir: &str,
        keys: AccountKeys,
        indexer_url: String,
        birthday_epoch: u64,
    ) -> anyhow::Result<Arc<Wallet>> {
        let store_path = PathBuf::from(data_dir).join(format!("{name}.ootle.json"));
        let store = match Store::load(&store_path)? {
            Some(s) => s,
            None => {
                let s = Store {
                    birthday_epoch,
                    ..Store::default()
                };
                s.save(&store_path)?;
                s
            },
        };
        let wallet = Arc::new(Wallet {
            address: keys.address(),
            keys,
            store_path,
            store: Mutex::new(store),
            indexer_url: RwLock::new(indexer_url),
        });
        lock(&WALLETS).insert(name.to_string(), wallet.clone());
        Ok(wallet)
    }

    pub fn get(name: &str) -> anyhow::Result<Arc<Wallet>> {
        lock(&WALLETS)
            .get(name)
            .cloned()
            .ok_or_else(|| anyhow!("wallet '{name}' is not open"))
    }

    pub fn close(name: &str) {
        lock(&WALLETS).remove(name);
    }

    pub fn set_indexer_url(&self, url: String) {
        *self.indexer_url.write().unwrap_or_else(|e| e.into_inner()) = url;
    }

    /// Read or mutate the store. Mutations are saved before the lock is released. Never hold this
    /// across an `.await`.
    pub fn read<T>(&self, f: impl FnOnce(&Store) -> T) -> T {
        f(&lock(&self.store))
    }

    pub fn write<T>(&self, f: impl FnOnce(&mut Store) -> T) -> anyhow::Result<T> {
        let mut store = lock(&self.store);
        let out = f(&mut store);
        store.save(&self.store_path)?;
        Ok(out)
    }

    /// A provider that verifies the indexer's answers against the validator set this wallet
    /// trusts, picking up where the last session left off.
    pub async fn provider(&self) -> anyhow::Result<Provider> {
        let url = self.indexer_url.read().unwrap_or_else(|e| e.into_inner()).clone();
        let provider = ProviderBuilder::new()
            .with_network(self.keys.network)
            .wallet(OotleWallet::from(PrivateKeyProvider::new(self.keys.secret())))
            .indexer_trust(IndexerTrust::Verify)
            .connect(&url)
            .await?;
        if let Some(saved) = self.read(|s| s.validator_set.clone()) {
            provider.restore_validator_set(saved).await?;
        }
        Ok(provider)
    }

    pub async fn remember_validator_set(&self, provider: &Provider) -> anyhow::Result<()> {
        if let Some(saved) = provider.saved_validator_set().await {
            self.write(|s| s.validator_set = Some(saved))?;
        }
        Ok(())
    }
}

/// A poisoned lock only means another thread panicked mid-operation; the store is saved after each
/// whole mutation, so carrying on with what is in memory is safe.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
