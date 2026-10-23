//! The formats gestalt reads besides JSON. Each one is turned into a
//! `serde_json::Value` and then described by the same collector, so the
//! rules about what is shown — nothing, unless asked by path — hold for all.

pub mod ini;
pub mod toml;
pub mod yaml;

pub use ini::Dialect;
