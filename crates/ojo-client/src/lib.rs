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
//! use ojo_client::{Tracer, TracerConfig, EventRecord, event_type};
//! use std::path::PathBuf;
//! use std::time::Duration;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a tracer configuration
//! let config = TracerConfig {
//!     output_dir: PathBuf::from("./traces"),
//!     buffer_size: 512 * 1024 * 1024, // 512 MiB
//!     flush_interval: Duration::from_secs(1),
//! };
//!
//! // Initialize the tracer
//! let tracer = Tracer::new(config)?;
//!
//! // Record events
//! tracer.record(EventRecord {
//!     ts_delta_ns: 12345,
//!     flow_id: 1,
//!     event_type: event_type::PACKET_SENT,
//!     payload: 100,
//! });
//!
//! tracer.record(EventRecord {
//!     ts_delta_ns: 12350,
//!     flow_id: 1,
//!     event_type: event_type::PACKET_ACKED,
//!     payload: 100,
//! });
//!
//! tracer.record(EventRecord {
//!     ts_delta_ns: 12360,
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
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zerocopy::{AsBytes, FromBytes, FromZeroes};

/// Maximum number of events to read from ring buffer per flush
const MAX_EVENTS_PER_FLUSH: usize = 100_000;

/// Binary file header (24 bytes)
#[repr(C)]
#[derive(Debug, Clone, Copy, AsBytes, FromBytes, FromZeroes)]
pub struct FileHeader {
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
/// This is the record format for both the API and on-disk storage
#[repr(C)]
#[derive(Debug, Clone, Copy, AsBytes, FromBytes, FromZeroes)]
pub struct EventRecord {
    pub ts_delta_ns: u64, // Time delta from batch_start_ns
    pub flow_id: u64,
    pub event_type: u64,
    pub payload: u64,
}

/// Lock-free ring buffer for event storage
struct RingBuffer {
    buffer: *mut EventRecord,
    write_head: AtomicUsize,
    read_head: AtomicUsize,
    dropped_count: AtomicU64,
    capacity: usize,      // Number of records (must be power of 2)
    capacity_mask: usize, // capacity - 1, for fast modulo
}

unsafe impl Send for RingBuffer {}
unsafe impl Sync for RingBuffer {}

impl RingBuffer {
    fn new(capacity_bytes: usize) -> Self {
        const RECORD_SIZE: usize = std::mem::size_of::<EventRecord>();
        
        // Calculate capacity in records
        let mut capacity = capacity_bytes / RECORD_SIZE;
        
        // Ensure capacity is at least 64 records
        if capacity < 64 {
            capacity = 64;
        }
        
        // Round up to next power of two
        capacity = capacity.next_power_of_two();
        
        let capacity_mask = capacity - 1;
        
        // Allocate buffer
        let layout = std::alloc::Layout::array::<EventRecord>(capacity).unwrap();
        let buffer = unsafe { std::alloc::alloc(layout) as *mut EventRecord };
        
        if buffer.is_null() {
            panic!("Failed to allocate ring buffer");
        }
        
        Self {
            buffer,
            write_head: AtomicUsize::new(0),
            read_head: AtomicUsize::new(0),
            dropped_count: AtomicU64::new(0),
            capacity,
            capacity_mask,
        }
    }

    /// Try to write an event record to the ring buffer
    /// Returns true if successful, false if buffer is full
    fn write(&self, record: &EventRecord) -> bool {
        loop {
            let write_pos = self.write_head.load(Ordering::Acquire);
            let read_pos = self.read_head.load(Ordering::Acquire);
            
            // Calculate used space (in records)
            let used = write_pos.wrapping_sub(read_pos);
            
            // Check if there's enough space (leave one slot to distinguish full from empty)
            if used >= self.capacity - 1 {
                // Buffer full, drop event
                self.dropped_count.fetch_add(1, Ordering::Relaxed);
                return false;
            }
            
            // Try to reserve space using CAS
            let new_write_pos = write_pos.wrapping_add(1);
            if self
                .write_head
                .compare_exchange(
                    write_pos,
                    new_write_pos,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_err()
            {
                // CAS failed, retry
                continue;
            }
            
            // Successfully reserved space, write the data
            let index = write_pos & self.capacity_mask;
            unsafe {
                std::ptr::write(self.buffer.add(index), *record);
            }
            
            // Release fence to ensure visibility
            std::sync::atomic::fence(Ordering::Release);
            return true;
        }
    }

    /// Read available events from the ring buffer using a callback
    /// The callback receives two slices: head and tail (tail may be empty)
    fn read<F>(&self, max_events: usize, mut callback: F)
    where
        F: FnMut(&[EventRecord]),
    {
        let write_pos = self.write_head.load(Ordering::Acquire);
        let read_pos = self.read_head.load(Ordering::Acquire);
        
        // Calculate available records
        let available = write_pos.wrapping_sub(read_pos);
        let to_read = available.min(max_events);
        
        if to_read == 0 {
            return;
        }
        
        // Create slice from buffer (no wrap-around with masking)
        let start_index = read_pos & self.capacity_mask;
        let end_index = (read_pos + to_read) & self.capacity_mask;
        
        unsafe {
            if end_index > start_index || end_index == 0 {
                // Contiguous read (or exactly at boundary)
                let slice = std::slice::from_raw_parts(self.buffer.add(start_index), to_read);
                callback(slice);
            } else {
                // Split read (wrapped around)
                let first_part = self.capacity - start_index;
                let second_part = to_read - first_part;
                
                let first_slice = std::slice::from_raw_parts(self.buffer.add(start_index), first_part);
                callback(first_slice);
                
                if second_part > 0 {
                    let second_slice = std::slice::from_raw_parts(self.buffer, second_part);
                    callback(second_slice);
                }
            }
        }
        
        // Update read head
        self.read_head.store(read_pos + to_read, Ordering::Release);
    }

    /// Get and reset the dropped event count
    fn take_dropped_count(&self) -> u64 {
        self.dropped_count.swap(0, Ordering::Relaxed)
    }
}

impl Drop for RingBuffer {
    fn drop(&mut self) {
        let layout = std::alloc::Layout::array::<EventRecord>(self.capacity).unwrap();
        unsafe {
            std::alloc::dealloc(self.buffer as *mut u8, layout);
        }
    }
}

/// Shared state between tracer and flusher thread
struct SharedState {
    ring_buffer: RingBuffer,
    batch_start_ns: u64,
    staging_dir: PathBuf,
    output_dir: PathBuf,
}

/// Configuration for the tracer
#[derive(Debug, Clone)]
pub struct TracerConfig {
    /// Directory where trace files will be written
    pub output_dir: PathBuf,

    /// Size of the ring buffer in bytes (default: 512 MiB)
    pub buffer_size: usize,

    /// Flush interval (default: 1 second)
    pub flush_interval: Duration,
}

impl Default for TracerConfig {
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("./traces"),
            buffer_size: 512 * 1024 * 1024, // 512 MiB
            flush_interval: Duration::from_secs(1),
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

    /// Set the flush interval
    pub fn with_flush_interval(mut self, interval: Duration) -> Self {
        self.flush_interval = interval;
        self
    }
}

/// Main tracer handle for recording events
pub struct Tracer {
    shared_state: Arc<SharedState>,
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
        let ring_buffer = RingBuffer::new(config.buffer_size);

        // Get batch start timestamp
        let batch_start_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        // Create shared state
        let shared_state = Arc::new(SharedState {
            ring_buffer,
            batch_start_ns,
            staging_dir,
            output_dir,
        });

        // Start flusher thread
        let flusher_state = shared_state.clone();
        let flush_interval = config.flush_interval;
        thread::spawn(move || {
            flusher_thread_main(flusher_state, flush_interval);
        });

        Ok(Self { shared_state })
    }

    /// Record a trace event
    ///
    /// The caller is responsible for populating the event structure,
    /// including the timestamp, flow_id, event_type, and payload.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use ojo_client::{Tracer, EventRecord, event_type};
    /// # let tracer = Tracer::new(Default::default()).unwrap();
    ///
    /// let event = EventRecord {
    ///     ts_delta_ns: 12345, // Time delta from batch start
    ///     flow_id: 1,
    ///     event_type: event_type::PACKET_SENT,
    ///     payload: 100, // packet number
    /// };
    /// tracer.record(event);
    /// ```
    pub fn record(&self, record: EventRecord) {
        // Write to ring buffer (lock-free)
        self.shared_state.ring_buffer.write(&record);
    }
}

/// Background flusher thread main function
fn flusher_thread_main(state: Arc<SharedState>, flush_interval: Duration) {
    let mut batch_counter = 0u64;

    loop {
        // Sleep for flush interval
        thread::sleep(flush_interval);

        // Check if we're the last reference (tracer dropped)
        if Arc::strong_count(&state) == 1 {
            // Do final flush and exit
            if let Err(e) = flush_events_from_ring_buffer(&state, &mut batch_counter) {
                eprintln!("Error flushing events to file: {}", e);
            }
            break;
        }

        // Flush events from ring buffer
        if let Err(e) = flush_events_from_ring_buffer(&state, &mut batch_counter) {
            eprintln!("Error flushing events to file: {}", e);
        }
    }
}

/// Flush events directly from ring buffer to a binary file
fn flush_events_from_ring_buffer(
    state: &SharedState,
    batch_counter: &mut u64,
) -> io::Result<()> {
    // Check for dropped events
    let dropped = state.ring_buffer.take_dropped_count();
    
    // Check if we have any data to write
    let write_pos = state.ring_buffer.write_head.load(Ordering::Acquire);
    let read_pos = state.ring_buffer.read_head.load(Ordering::Acquire);
    let available = write_pos.wrapping_sub(read_pos);
    
    if available == 0 && dropped == 0 {
        return Ok(());
    }
    
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

    // Write events directly from ring buffer using callback
    state.ring_buffer.read(MAX_EVENTS_PER_FLUSH, |slice| {
        // Convert slice of EventRecords to bytes and write
        let bytes = unsafe {
            std::slice::from_raw_parts(
                slice.as_ptr() as *const u8,
                slice.len() * std::mem::size_of::<EventRecord>(),
            )
        };
        if let Err(e) = file.write_all(bytes) {
            eprintln!("Error writing events to file: {}", e);
        }
    });

    // Write dropped events record if any
    if dropped > 0 {
        let dropped_event = EventRecord {
            ts_delta_ns: 0,
            flow_id: 0,
            event_type: event_type::EVENTS_DROPPED,
            payload: dropped,
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
        assert_eq!(config.flush_interval, Duration::from_secs(1));
    }

    #[test]
    fn test_config_builder() {
        let config = TracerConfig::default()
            .with_output_dir("/tmp/traces")
            .with_buffer_size(1024 * 1024)
            .with_flush_interval(Duration::from_millis(500));

        assert_eq!(config.output_dir, PathBuf::from("/tmp/traces"));
        assert_eq!(config.buffer_size, 1024 * 1024);
        assert_eq!(config.flush_interval, Duration::from_millis(500));
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
            .with_flush_interval(Duration::from_millis(100));

        let tracer = Tracer::new(config).unwrap();

        // Record some events
        for i in 0..10 {
            tracer.record(EventRecord {
                ts_delta_ns: i * 1000,
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
        let buffer = RingBuffer::new(1024 * 1024);

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

        // Read events back using callback
        let mut events = Vec::new();
        buffer.read(10, |slice| {
            events.extend_from_slice(slice);
        });
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
        // Create a small buffer
        const RECORD_SIZE: usize = std::mem::size_of::<EventRecord>();
        let buffer = RingBuffer::new(RECORD_SIZE * 10);

        // The buffer will hold at least 64 records (minimum), so fill more
        let capacity = 63; // Leave one slot
        for i in 0..capacity {
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
        let mut read_count = 0;
        buffer.read(10, |slice| {
            read_count += slice.len();
        });
        assert_eq!(read_count, 10);
        
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
            .with_flush_interval(Duration::from_millis(100));

        let tracer = Tracer::new(config).unwrap();

        // Simulate a packet lifecycle
        let flow_id = 42;
        
        tracer.record(EventRecord {
            ts_delta_ns: 0,
            flow_id,
            event_type: event_type::PACKET_CREATED,
            payload: 1,
        });

        tracer.record(EventRecord {
            ts_delta_ns: 1000,
            flow_id,
            event_type: event_type::PACKET_SENT,
            payload: 1,
        });

        tracer.record(EventRecord {
            ts_delta_ns: 2000,
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
            .with_flush_interval(Duration::from_millis(200));

        let tracer = Arc::new(Tracer::new(config).unwrap());

        // Spawn multiple threads recording events
        let mut handles = vec![];
        for thread_id in 0..4 {
            let tracer_clone = tracer.clone();
            let handle = thread::spawn(move || {
                for i in 0..100 {
                    tracer_clone.record(EventRecord {
                        ts_delta_ns: i * 1000,
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
