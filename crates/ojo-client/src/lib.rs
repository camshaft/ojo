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

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zerocopy::{AsBytes, FromBytes, FromZeroes};

/// Maximum number of events to read from ring buffer per flush
const MAX_EVENTS_PER_FLUSH: usize = 100_000;

/// Binary file header (24 bytes)
#[repr(C)]
#[derive(Debug, Clone, Copy, AsBytes, FromBytes, FromZeroes)]
struct FileHeader {
    magic: [u8; 4],      // b"ojo\0"
    version: u8,         // 1
    reserved: [u8; 3],   // [0, 0, 0]
    batch_start_ns: u64, // Unix timestamp in nanoseconds
    _reserved2: u64,     // 0
}

impl FileHeader {
    fn new(batch_start_ns: u64) -> Self {
        Self {
            magic: *b"ojo\0",
            version: 1,
            reserved: [0, 0, 0],
            batch_start_ns,
            _reserved2: 0,
        }
    }
}

/// Binary event record (32 bytes)
#[repr(C)]
#[derive(Debug, Clone, Copy, AsBytes, FromBytes, FromZeroes)]
struct EventRecord {
    ts_delta_ns: u64, // Time delta from batch_start_ns
    flow_id: u64,
    event_type: u64,
    payload: u64,
}

/// Lock-free ring buffer for event storage
struct RingBuffer {
    buffer: Arc<Box<[u8]>>,
    write_head: AtomicUsize,
    read_head: AtomicUsize,
    dropped_count: AtomicU64,
    capacity: usize,
}

impl RingBuffer {
    fn new(capacity: usize) -> Self {
        // Allocate buffer without unnecessary initialization
        let buffer = vec![0u8; capacity].into_boxed_slice();
        Self {
            buffer: Arc::new(buffer),
            write_head: AtomicUsize::new(0),
            read_head: AtomicUsize::new(0),
            dropped_count: AtomicU64::new(0),
            capacity,
        }
    }

    /// Try to write an event record to the ring buffer
    /// Returns true if successful, false if buffer is full
    fn write(&self, record: &EventRecord) -> bool {
        const RECORD_SIZE: usize = std::mem::size_of::<EventRecord>();
        
        loop {
            let write_pos = self.write_head.load(Ordering::Acquire);
            let read_pos = self.read_head.load(Ordering::Acquire);
            
            // Calculate used space
            let used = if write_pos >= read_pos {
                write_pos - read_pos
            } else {
                self.capacity - read_pos + write_pos
            };
            
            // Calculate available space (leave one record space as buffer to distinguish full from empty)
            let available = self.capacity - used;
            
            // Check if there's enough space (need RECORD_SIZE + extra to avoid confusion with empty buffer)
            if available <= RECORD_SIZE {
                // Buffer full, drop event
                self.dropped_count.fetch_add(1, Ordering::Relaxed);
                return false;
            }
            
            // Try to reserve space using CAS
            let new_write_pos = (write_pos + RECORD_SIZE) % self.capacity;
            if self
                .write_head
                .compare_exchange(
                    write_pos,
                    new_write_pos,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                // Successfully reserved space, now write the data
                let bytes = record.as_bytes();
                let buffer_ptr = self.buffer.as_ptr() as *mut u8;
                
                // Handle wrap-around
                let end = write_pos + RECORD_SIZE;
                if end <= self.capacity {
                    // No wrap-around
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            bytes.as_ptr(),
                            buffer_ptr.add(write_pos),
                            RECORD_SIZE,
                        );
                    }
                } else {
                    // Wrap-around case
                    let first_part = self.capacity - write_pos;
                    let second_part = RECORD_SIZE - first_part;
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            bytes.as_ptr(),
                            buffer_ptr.add(write_pos),
                            first_part,
                        );
                        std::ptr::copy_nonoverlapping(
                            bytes.as_ptr().add(first_part),
                            buffer_ptr,
                            second_part,
                        );
                    }
                }
                
                // Release fence to ensure visibility
                std::sync::atomic::fence(Ordering::Release);
                return true;
            }
            // CAS failed, retry
        }
    }

    /// Read available events from the ring buffer
    fn read(&self, max_events: usize) -> Vec<EventRecord> {
        const RECORD_SIZE: usize = std::mem::size_of::<EventRecord>();
        
        let mut events = Vec::new();
        let write_pos = self.write_head.load(Ordering::Acquire);
        let mut read_pos = self.read_head.load(Ordering::Acquire);
        
        while events.len() < max_events {
            if read_pos == write_pos {
                break; // No more data available
            }
            
            let mut record = EventRecord::new_zeroed();
            let bytes = record.as_bytes_mut();
            
            // Handle wrap-around
            let end = read_pos + RECORD_SIZE;
            if end <= self.capacity {
                // No wrap-around
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        self.buffer.as_ptr().add(read_pos),
                        bytes.as_mut_ptr(),
                        RECORD_SIZE,
                    );
                }
            } else {
                // Wrap-around case
                let first_part = self.capacity - read_pos;
                let second_part = RECORD_SIZE - first_part;
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        self.buffer.as_ptr().add(read_pos),
                        bytes.as_mut_ptr(),
                        first_part,
                    );
                    std::ptr::copy_nonoverlapping(
                        self.buffer.as_ptr(),
                        bytes.as_mut_ptr().add(first_part),
                        second_part,
                    );
                }
            }
            
            events.push(record);
            read_pos = (read_pos + RECORD_SIZE) % self.capacity;
        }
        
        // Update read head
        if !events.is_empty() {
            self.read_head.store(read_pos, Ordering::Release);
        }
        
        events
    }

    /// Get and reset the dropped event count
    fn take_dropped_count(&self) -> u64 {
        self.dropped_count.swap(0, Ordering::Relaxed)
    }
}

/// Shared state between tracer and flusher thread
struct SharedState {
    ring_buffer: Arc<RingBuffer>,
    batch_start_ns: u64,
    staging_dir: PathBuf,
    output_dir: PathBuf,
    shutdown: AtomicBool,
    flush_signal: Arc<(Mutex<bool>, Condvar)>,
}

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
    shared_state: Arc<SharedState>,
    flusher_thread: Option<JoinHandle<()>>,
}

impl Tracer {
    /// Create a new tracer with the given configuration
    ///
    /// This initializes the ring buffer, creates the staging and output directories,
    /// and starts the background flusher thread.
    pub fn new(config: TracerConfig) -> Result<Self, std::io::Error> {
        // Create staging/ and output/ directories
        let staging_dir = config.output_dir.join("staging");
        let output_dir = config.output_dir.join("output");
        fs::create_dir_all(&staging_dir)?;
        fs::create_dir_all(&output_dir)?;

        // Initialize ring buffer
        let ring_buffer = Arc::new(RingBuffer::new(config.buffer_size));

        // Get batch start timestamp
        let batch_start_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        // Create shared state
        let shared_state = Arc::new(SharedState {
            ring_buffer: ring_buffer.clone(),
            batch_start_ns,
            staging_dir,
            output_dir,
            shutdown: AtomicBool::new(false),
            flush_signal: Arc::new((Mutex::new(false), Condvar::new())),
        });

        // Start flusher thread
        let flusher_state = shared_state.clone();
        let flush_interval = Duration::from_millis(config.flush_interval_ms);
        let flusher_thread = thread::spawn(move || {
            flusher_thread_main(flusher_state, flush_interval);
        });

        Ok(Self {
            shared_state,
            flusher_thread: Some(flusher_thread),
        })
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
        // Convert Event to EventRecord with timestamp delta
        let ts_delta_ns = event
            .timestamp_ns
            .saturating_sub(self.shared_state.batch_start_ns);

        let record = EventRecord {
            ts_delta_ns,
            flow_id: event.flow_id,
            event_type: event.event_type,
            payload: event.payload,
        };

        // Write to ring buffer (lock-free)
        self.shared_state.ring_buffer.write(&record);
    }
}

impl Drop for Tracer {
    fn drop(&mut self) {
        // Signal flusher thread to stop
        self.shared_state.shutdown.store(true, Ordering::Release);

        // Wake up flusher thread
        let (lock, cvar) = &*self.shared_state.flush_signal;
        {
            let mut signaled = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            *signaled = true;
        }
        cvar.notify_one();

        // Wait for flusher thread to finish
        if let Some(handle) = self.flusher_thread.take() {
            let _ = handle.join();
        }
    }
}

/// Background flusher thread main function
fn flusher_thread_main(state: Arc<SharedState>, flush_interval: Duration) {
    let mut batch_counter = 0u64;

    loop {
        // Wait for flush interval or shutdown signal
        let (lock, cvar) = &*state.flush_signal;
        let lock_result = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let wait_result = cvar.wait_timeout_while(
            lock_result,
            flush_interval,
            |&mut signaled| !signaled && !state.shutdown.load(Ordering::Acquire),
        );
        
        // Handle potential error from wait_timeout
        if wait_result.is_err() {
            // If wait failed, we should still continue to flush and check shutdown
            eprintln!("Warning: Condition variable wait failed, continuing with flush");
        }

        // Check if we should shutdown
        let should_shutdown = state.shutdown.load(Ordering::Acquire);

        // Read events from ring buffer
        let events = state.ring_buffer.read(MAX_EVENTS_PER_FLUSH);

        // Check for dropped events
        let dropped = state.ring_buffer.take_dropped_count();

        // Only write a file if we have events or dropped count
        if !events.is_empty() || dropped > 0 {
            if let Err(e) = flush_events_to_file(&state, &events, dropped, &mut batch_counter) {
                eprintln!("Error flushing events to file: {}", e);
            }
        }

        // Exit if shutdown was requested
        if should_shutdown {
            break;
        }
    }
}

/// Flush events to a binary file
fn flush_events_to_file(
    state: &SharedState,
    events: &[EventRecord],
    dropped_count: u64,
    batch_counter: &mut u64,
) -> io::Result<()> {
    // Generate unique filename
    let filename = format!("trace_{:016x}_{:08x}.bin", state.batch_start_ns, *batch_counter);
    *batch_counter += 1;

    let staging_path = state.staging_dir.join(&filename);
    let output_path = state.output_dir.join(&filename);

    // Write to staging file
    let mut file = File::create(&staging_path)?;

    // Write header
    let header = FileHeader::new(state.batch_start_ns);
    file.write_all(header.as_bytes())?;

    // Write events
    for event in events {
        file.write_all(event.as_bytes())?;
    }

    // Write dropped events record if any
    if dropped_count > 0 {
        let dropped_event = EventRecord {
            ts_delta_ns: 0,
            flow_id: 0,
            event_type: event_type::EVENTS_DROPPED,
            payload: dropped_count,
        };
        file.write_all(dropped_event.as_bytes())?;
    }

    // Ensure all data is written
    file.sync_all()?;
    drop(file);

    // Atomic move to output directory
    fs::rename(staging_path, output_path)?;

    Ok(())
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
    use std::fs;
    use std::thread;
    use std::time::Duration;
    use tempfile::TempDir;

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

    #[test]
    fn test_tracer_initialization() {
        let temp_dir = TempDir::new().unwrap();
        let config = TracerConfig::default()
            .with_output_dir(temp_dir.path())
            .with_buffer_size(1024 * 1024);

        let tracer = Tracer::new(config).unwrap();
        
        // Check that directories were created
        assert!(temp_dir.path().join("staging").exists());
        assert!(temp_dir.path().join("output").exists());

        drop(tracer);
    }

    #[test]
    fn test_event_recording_and_flush() {
        let temp_dir = TempDir::new().unwrap();
        let config = TracerConfig::default()
            .with_output_dir(temp_dir.path())
            .with_buffer_size(1024 * 1024)
            .with_flush_interval_ms(100); // Fast flush for testing

        let batch_start = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        let tracer = Tracer::new(config).unwrap();

        // Record some events
        for i in 0..10 {
            tracer.record(Event {
                timestamp_ns: batch_start + i * 1000,
                flow_id: 1,
                event_type: event_type::PACKET_SENT,
                payload: i,
            });
        }

        // Wait for flush
        thread::sleep(Duration::from_millis(300));

        // Check that files were created
        let output_dir = temp_dir.path().join("output");
        let entries: Vec<_> = fs::read_dir(output_dir)
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        
        assert!(
            !entries.is_empty(),
            "Expected at least one trace file to be created"
        );

        drop(tracer);
    }

    #[test]
    fn test_ring_buffer_write_and_read() {
        let buffer = RingBuffer::new(1024);

        // Write some events
        for i in 0..10 {
            let record = EventRecord {
                ts_delta_ns: i * 1000,
                flow_id: 1,
                event_type: event_type::PACKET_SENT,
                payload: i,
            };
            assert!(buffer.write(&record));
        }

        // Read events back
        let events = buffer.read(10);
        assert_eq!(events.len(), 10);

        // Verify event data
        for (i, event) in events.iter().enumerate() {
            assert_eq!(event.ts_delta_ns, i as u64 * 1000);
            assert_eq!(event.flow_id, 1);
            assert_eq!(event.event_type, event_type::PACKET_SENT);
            assert_eq!(event.payload, i as u64);
        }
    }

    #[test]
    fn test_ring_buffer_overflow() {
        // Create a small buffer that can only hold a few events
        const RECORD_SIZE: usize = std::mem::size_of::<EventRecord>();
        let buffer = RingBuffer::new(RECORD_SIZE * 5);

        // Fill the buffer (we can write 4 records before it's full, leaving space to distinguish full from empty)
        for i in 0..4 {
            let record = EventRecord {
                ts_delta_ns: i,
                flow_id: 1,
                event_type: event_type::PACKET_SENT,
                payload: i,
            };
            assert!(buffer.write(&record), "Failed to write record {}", i);
        }

        // Try to write more - should fail since buffer is now full
        let record = EventRecord {
            ts_delta_ns: 100,
            flow_id: 1,
            event_type: event_type::PACKET_SENT,
            payload: 100,
        };
        assert!(!buffer.write(&record));

        // Check dropped count
        assert_eq!(buffer.take_dropped_count(), 1);
        
        // Read some events to make space
        let events = buffer.read(2);
        assert_eq!(events.len(), 2);
        
        // Now we should be able to write again
        let record = EventRecord {
            ts_delta_ns: 200,
            flow_id: 1,
            event_type: event_type::PACKET_SENT,
            payload: 200,
        };
        assert!(buffer.write(&record), "Should be able to write after reading");
    }

    #[test]
    fn test_file_header_format() {
        let batch_start = 123456789u64;
        let header = FileHeader::new(batch_start);

        assert_eq!(&header.magic, b"ojo\0");
        assert_eq!(header.version, 1);
        assert_eq!(header.batch_start_ns, batch_start);
    }

    #[test]
    fn test_binary_format_size() {
        // Verify expected sizes
        assert_eq!(std::mem::size_of::<FileHeader>(), 24);
        assert_eq!(std::mem::size_of::<EventRecord>(), 32);
    }

    #[test]
    fn test_multiple_events_same_flow() {
        let temp_dir = TempDir::new().unwrap();
        let config = TracerConfig::default()
            .with_output_dir(temp_dir.path())
            .with_buffer_size(1024 * 1024)
            .with_flush_interval_ms(100);

        let batch_start = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        let tracer = Tracer::new(config).unwrap();

        // Simulate a packet lifecycle
        let flow_id = 42;
        
        tracer.record(Event {
            timestamp_ns: batch_start,
            flow_id,
            event_type: event_type::PACKET_CREATED,
            payload: 1,
        });

        tracer.record(Event {
            timestamp_ns: batch_start + 1000,
            flow_id,
            event_type: event_type::PACKET_SENT,
            payload: 1,
        });

        tracer.record(Event {
            timestamp_ns: batch_start + 2000,
            flow_id,
            event_type: event_type::PACKET_ACKED,
            payload: 1,
        });

        // Wait for flush
        thread::sleep(Duration::from_millis(300));

        drop(tracer);
    }

    #[test]
    fn test_concurrent_event_recording() {
        let temp_dir = TempDir::new().unwrap();
        let config = TracerConfig::default()
            .with_output_dir(temp_dir.path())
            .with_buffer_size(10 * 1024 * 1024)
            .with_flush_interval_ms(200);

        let batch_start = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        let tracer = Arc::new(Tracer::new(config).unwrap());

        // Spawn multiple threads recording events
        let mut handles = vec![];
        for thread_id in 0..4 {
            let tracer_clone = tracer.clone();
            let handle = thread::spawn(move || {
                for i in 0..100 {
                    tracer_clone.record(Event {
                        timestamp_ns: batch_start + i * 1000,
                        flow_id: thread_id,
                        event_type: event_type::PACKET_SENT,
                        payload: i,
                    });
                }
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }

        // Wait for flush
        thread::sleep(Duration::from_millis(500));

        drop(tracer);
    }
}
