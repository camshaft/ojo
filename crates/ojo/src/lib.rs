//! # ojo
//!
//! Combined watcher and explorer for ojo trace files.
//!
//! ## Features
//!
//! - Watch directories for new trace files and transform them
//! - Serve a web interface for exploring and visualizing traces

use std::path::PathBuf;

pub use anyhow::{Error, Result};

/// Configuration for the watcher
#[derive(Debug, Clone)]
pub struct WatcherConfig {
    /// Directory to watch for trace files
    pub input_dir: PathBuf,

    /// Path to the database
    pub db_path: PathBuf,

    /// Age threshold for cleaning up old trace files (in seconds)
    pub cleanup_age_secs: Option<u64>,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            input_dir: PathBuf::from("./traces/output"),
            db_path: PathBuf::from("./traces.db"),
            cleanup_age_secs: None,
        }
    }
}

/// Main watcher instance
pub struct Watcher {
    _config: WatcherConfig,
}

impl Watcher {
    /// Create a new watcher with the given configuration
    pub fn new(config: WatcherConfig) -> Result<Self> {
        // TODO: Implement watcher initialization
        // - Open/create database
        // - Initialize schema
        // - Set up file system watcher

        Ok(Self { _config: config })
    }

    /// Process all existing trace files in the input directory
    pub async fn process_existing_files(&self) -> Result<()> {
        // TODO: Scan directory and process all .bin files
        Ok(())
    }

    /// Start watching for new trace files (blocking)
    pub async fn watch(&self) -> Result<()> {
        // TODO: Set up file watcher and process files as they appear
        Ok(())
    }
}

/// Configuration for the explorer server
#[derive(Debug, Clone)]
pub struct ExplorerConfig {
    /// Path to the database
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
    fn test_watcher_config_default() {
        let config = WatcherConfig::default();
        assert!(config.cleanup_age_secs.is_none());
    }

    #[test]
    fn test_explorer_config_default() {
        let config = ExplorerConfig::default();
        assert_eq!(config.port, 8080);
        assert_eq!(config.host, "127.0.0.1");
    }
}
