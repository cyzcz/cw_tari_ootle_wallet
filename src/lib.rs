pub mod api;
pub(crate) mod domain;
mod frb_generated;

#[flutter_rust_bridge::frb(init)]
pub fn init_app() {
    flutter_rust_bridge::setup_default_user_utils();
}
