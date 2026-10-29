//! reqwest's rustls checks certificates through `rustls-platform-verifier`, which on Android must
//! be handed the app's Context over JNI before the first HTTPS request, or every indexer call
//! panics. The host app's plugin calls this once, right after loading the library, and logs a
//! `false` return.
//!
//! On an x86_64 emulator running the arm64 build through ARM translation these JNI calls fail; use
//! an x86_64 build there.

use jni::{
    objects::{JClass, JObject},
    sys::jboolean,
    EnvUnowned,
};

#[no_mangle]
pub extern "system" fn Java_com_cakewallet_cw_1tari_1ootle_CwTariOotlePlugin_initTls<'caller>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    context: JObject<'caller>,
) -> jboolean {
    // A failed init resolves to the default, false.
    unowned_env
        .with_env(|env| rustls_platform_verifier::android::init_with_env(env, context).map(|()| true))
        .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}
