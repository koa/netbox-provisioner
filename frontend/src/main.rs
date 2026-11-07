//! Scope: The frontend's module tree and its start; nothing else.
use crate::app::App;
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};
mod app;
mod auth;
pub mod components;
mod error;
mod graphql;
mod i18n;
mod local_storage;
pub mod pages;
mod routes;
mod svg_export;
mod target;

#[wasm_bindgen]
pub fn init_panic_hook() {
    console_error_panic_hook::set_once();
}
#[cfg(not(debug_assertions))]
const LOG_LEVEL: log::Level = log::Level::Info;
#[cfg(debug_assertions)]
const LOG_LEVEL: log::Level = log::Level::Trace;
pub fn main() -> Result<(), JsValue> {
    wasm_logger::init(wasm_logger::Config::new(LOG_LEVEL));
    yew::Renderer::<App>::new().render();
    Ok(())
}
