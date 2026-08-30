//! Runtime-reloadable log filter — REQ-PICOO-UI-002 / PRD §16 General.

use std::sync::{Mutex, OnceLock};

use tracing_subscriber::prelude::*;
use tracing_subscriber::{fmt, reload, EnvFilter, Registry};

type FilterHandle = reload::Handle<EnvFilter, Registry>;

static FILTER_RELOAD: OnceLock<Mutex<FilterHandle>> = OnceLock::new();

/// Initialize the global subscriber once. Prefer `default_filter` (from prefs)
/// over a bare `RUST_LOG` when provided.
pub fn init_logging(default_filter: &str) {
    let filter = EnvFilter::try_new(default_filter)
        .or_else(|_| EnvFilter::try_from_default_env())
        .unwrap_or_else(|_| EnvFilter::new("info"));
    let (filter_layer, handle) = reload::Layer::new(filter);
    let _ = FILTER_RELOAD.set(Mutex::new(handle));
    // Ignore double-init in tests / CLI re-entry.
    let _ = tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt::layer())
        .try_init();
}

/// Reload the EnvFilter without restarting the process (通用 → 日志级别).
#[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
pub fn reload_filter(filter: &str) -> Result<(), String> {
    let handle = FILTER_RELOAD
        .get()
        .ok_or_else(|| "logging not initialized".to_string())?;
    let guard = handle
        .lock()
        .map_err(|_| "logging reload lock poisoned".to_string())?;
    let new_filter =
        EnvFilter::try_new(filter).map_err(|err| format!("invalid log filter {filter}: {err}"))?;
    guard
        .reload(new_filter)
        .map_err(|err| format!("reload log filter: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::LogLevel;

    #[test]
    fn log_level_env_filters_are_valid_env_filters() {
        for level in LogLevel::ALL {
            EnvFilter::try_new(level.env_filter())
                .unwrap_or_else(|err| panic!("{}: {err}", level.env_filter()));
        }
    }

    #[test]
    fn reload_after_init_accepts_debug() {
        init_logging("info");
        reload_filter("debug").expect("reload debug");
        reload_filter("warn").expect("reload warn");
    }
}
