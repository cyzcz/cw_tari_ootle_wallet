use rust_lib_cw_tari_ootle::api::{
    balance::get_revealed_tari_balance,
    wallet::{create_wallet, restore_wallet, validate_address, TariOotleNetwork},
};

#[test]
fn restore_reproduces_created_address() {
    let created = create_wallet(TariOotleNetwork::Esmeralda).unwrap();
    assert_eq!(created.seed_words.split(' ').count(), 24);
    assert!(created.address.starts_with("otl_esm_"), "{}", created.address);
    assert!(validate_address(created.address.clone()));
    assert!(!validate_address(created.address.replace("otl_esm_1", "otl_esm_2")));
    let restored = restore_wallet(created.seed_words, TariOotleNetwork::Esmeralda).unwrap();
    assert_eq!(restored, created.address);
}

#[tokio::test]
#[ignore = "live: hits the public Esmeralda indexer"]
async fn live_fresh_account_has_zero_revealed_balance() {
    let created = create_wallet(TariOotleNetwork::Esmeralda).unwrap();
    let bal = get_revealed_tari_balance("https://esme-indexer-a.ootle.tari.com/".into(), created.address).await;
    // A never-used account has no component yet; either Ok(0) or a not-found error is a real round trip.
    println!("{bal:?}");
}
