//! # ojo-explorer
//!
//! Browser-based explorer for querying and visualizing ojo trace data.
//!
//! ## Features
//!
//! - REST API for querying trace data
//! - Real-time updates
//! - Export capabilities

use std::path::PathBuf;

pub use anyhow::{Error, Result};

/// Configuration for the explorer server
#[derive(Debug, Clone)]
pub struct ExplorerConfig {
    /// Path to the SQLite database
    pub db_path: PathBuf,

    /// Port to bind the web server to
    pub port: u16,

    /// Host to bind to
    pub host: String,
}

impl Default for ExplorerConfig {
    fn default() -> Self {
        Self {
            db_path: PathBuf::from("./traces.db"),
            port: 8080,
            host: "127.0.0.1".to_string(),
        }
    }
}

/// Main explorer server instance
pub struct Explorer {
    _config: ExplorerConfig,
}

impl Explorer {
    /// Create a new explorer with the given configuration
    pub fn new(config: ExplorerConfig) -> Result<Self> {
        // TODO: Implement explorer initialization
        Ok(Self { _config: config })
    }

    /// Start the web server (blocking)
    pub async fn serve(&self) -> Result<()> {
        // TODO: Set up Axum server with routes
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = ExplorerConfig::default();
        assert_eq!(config.port, 8080);
        assert_eq!(config.host, "127.0.0.1");
    }
}
