//! # ojo-watcher
//!
//! Watcher daemon for monitoring trace directories and transforming
//! binary trace files into a queryable SQLite database.
//!
//! ## Features
//!
//! - Directory monitoring with real-time file detection
//! - Binary trace file parsing
//! - SQLite database transformation
//! - Flow hierarchy reconstruction
//! - Dropped event detection and logging

use std::path::PathBuf;

pub use anyhow::{Result, Error};

/// Configuration for the watcher
#[derive(Debug, Clone)]
pub struct WatcherConfig {
    /// Directory to watch for trace files
    pub input_dir: PathBuf,
    
    /// Path to the SQLite database
    pub db_path: PathBuf,
    
    /// Whether to continuously watch for new files
    pub watch: bool,
    
    /// Age threshold for cleaning up old trace files (in seconds)
    pub cleanup_age_secs: Option<u64>,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            input_dir: PathBuf::from("./traces/output"),
            db_path: PathBuf::from("./traces.db"),
            watch: false,
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
        // - Open/create SQLite database
        // - Initialize schema
        // - Set up file system watcher if watch mode enabled
        
        Ok(Self {
            _config: config,
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = WatcherConfig::default();
        assert_eq!(config.watch, false);
        assert!(config.cleanup_age_secs.is_none());
    }
}
