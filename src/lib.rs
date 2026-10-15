//! Delta: one API for nodes, components and capabilities.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const PROTOCOL: &str = "v1";

pub mod auth;
pub mod client;
pub mod components;
pub mod config;
pub mod errors;
pub mod kernel;
pub mod node;
pub mod plugin;
pub mod server;
pub mod util;
