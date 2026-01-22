//! # ojo-client
//!
//! Low-overhead transport protocol event tracing client library.
//!
//! This library provides a lock-free, thread-safe event recording system
//! for capturing transport protocol events with minimal overhead.
//!
//! ## Features
//!
//! - **Lock-free**: Zero-allocation event recording with < 100ns per event
//! - **Thread-safe**: Multi-writer, single-reader architecture
//! - **Binary format**: Fixed 24-byte records for zero-copy parsing
//! - **Streaming**: No pre-known event count, suitable for long-running traces
//!
//! ## Example
//!
//! ```rust,no_run
//! use ojo_client::{Tracer, TracerConfig, Event, event_type};
//! use std::path::PathBuf;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a tracer configuration
//! let config = TracerConfig {
//!     output_dir: PathBuf::from("./traces"),
//!     buffer_size: 512 * 1024 * 1024, // 512 MiB
//!     flush_interval_ms: 1000,         // 1 second
//! };
//!
//! // Initialize the tracer
//! let tracer = Tracer::new(config)?;
//!
//! // Record events
//! tracer.record(Event {
//!     timestamp_ns: 12345,
//!     flow_id: 1,
//!     event_type: event_type::PACKET_SENT,
//!     payload: 100,
//! });
//!
//! tracer.record(Event {
//!     timestamp_ns: 12350,
//!     flow_id: 1,
//!     event_type: event_type::PACKET_ACKED,
//!     payload: 100,
//! });
//!
//! tracer.record(Event {
//!     timestamp_ns: 12360,
//!     flow_id: 1,
//!     event_type: event_type::STREAM_OPENED,
//!     payload: 10,
//! });
//!
//! // Tracer automatically flushes on drop
//! # Ok(())
//! # }
//! ```

use std::path::PathBuf;

/// Configuration for the tracer
#[derive(Debug, Clone)]
pub struct TracerConfig {
    /// Directory where trace files will be written
    pub output_dir: PathBuf,

    /// Size of the ring buffer in bytes (default: 512 MiB)
    pub buffer_size: usize,

    /// Flush interval in milliseconds (default: 1000ms)
    pub flush_interval_ms: u64,
}

impl Default for TracerConfig {
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("./traces"),
            buffer_size: 512 * 1024 * 1024, // 512 MiB
            flush_interval_ms: 1000,        // 1 second
        }
    }
}

impl TracerConfig {
    /// Create a new tracer configuration with the specified output directory
    pub fn new(output_dir: impl Into<PathBuf>) -> Self {
        Self {
            output_dir: output_dir.into(),
            ..Default::default()
        }
    }

    /// Set the output directory
    pub fn with_output_dir(mut self, output_dir: impl Into<PathBuf>) -> Self {
        self.output_dir = output_dir.into();
        self
    }

    /// Set the buffer size in bytes
    pub fn with_buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }

    /// Set the flush interval in milliseconds
    pub fn with_flush_interval_ms(mut self, interval: u64) -> Self {
        self.flush_interval_ms = interval;
        self
    }
}

/// Trace event structure
#[derive(Debug, Clone, Copy)]
pub struct Event {
    /// Timestamp in nanoseconds since tracer start
    pub timestamp_ns: u64,
    /// Flow identifier (unique per batch)
    pub flow_id: u64,
    /// Event type identifier
    pub event_type: u64,
    /// Event payload
    pub payload: u64,
}

/// Main tracer handle for recording events
pub struct Tracer {
    _config: TracerConfig,
}

impl Tracer {
    /// Create a new tracer with the given configuration
    ///
    /// This initializes the ring buffer, creates the staging and output directories,
    /// and starts the background flusher thread.
    pub fn new(config: TracerConfig) -> Result<Self, std::io::Error> {
        // TODO: Implement tracer initialization
        // - Create staging/ and output/ directories
        // - Initialize ring buffer
        // - Start flusher thread

        Ok(Self { _config: config })
    }

    /// Record a trace event
    ///
    /// The caller is responsible for populating the event structure,
    /// including the timestamp, flow_id, event_type, and payload.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use ojo_client::{Tracer, Event, event_type};
    /// # let tracer = Tracer::new(Default::default()).unwrap();
    ///
    /// let event = Event {
    ///     timestamp_ns: 12345, // Get from monotonic clock
    ///     flow_id: 1,
    ///     event_type: event_type::PACKET_SENT,
    ///     payload: 100, // packet number
    /// };
    /// tracer.record(event);
    /// ```
    pub fn record(&self, event: Event) {
        // TODO: Implement event recording
        let _ = event;
    }
}

impl Drop for Tracer {
    fn drop(&mut self) {
        // TODO: Signal flusher thread to stop and wait for final flush
    }
}

/// Metadata for an event type
#[derive(Debug, Clone, Copy)]
pub struct EventTypeInfo {
    /// The event type value
    pub value: u64,
    /// The constant name
    pub name: &'static str,
    /// The category (Packet, Stream, FlowControl, CongestionControl, Meta)
    pub category: &'static str,
    /// Human-readable description
    pub description: &'static str,
}

// Include generated event types
include!(concat!(env!("OUT_DIR"), "/event_types.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = TracerConfig::default();
        assert_eq!(config.buffer_size, 512 * 1024 * 1024);
        assert_eq!(config.flush_interval_ms, 1000);
    }

    #[test]
    fn test_config_builder() {
        let config = TracerConfig::default()
            .with_output_dir("/tmp/traces")
            .with_buffer_size(1024 * 1024)
            .with_flush_interval_ms(500);

        assert_eq!(config.output_dir, PathBuf::from("/tmp/traces"));
        assert_eq!(config.buffer_size, 1024 * 1024);
        assert_eq!(config.flush_interval_ms, 500);
    }
}
