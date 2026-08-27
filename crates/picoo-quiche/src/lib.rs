//! quiche build adaptation — REQ-PICOO-TRANSPORT-005.
//!
//! Only this crate and `picoo-transport` may depend on the `quiche` crate.

mod config;

pub use config::{build_client_config, build_server_config, QuicConfigError};

/// Re-export protocol version constant used when creating quiche configs.
pub use quiche;
