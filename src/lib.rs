// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

#[cfg(target_os = "android")]
mod android;
pub mod api;
pub(crate) mod domain;
mod frb_generated;

#[flutter_rust_bridge::frb(init)]
pub fn init_app() {
    flutter_rust_bridge::setup_default_user_utils();
}
