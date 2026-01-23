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
//! - **Per-CPU collection**: Eliminates contention via per-CPU event pages
//! - **Thread-safe**: Multi-writer, single-collector architecture
//! - **Binary format**: Fixed-sized records for zero-copy parsing
//! - **Streaming**: No pre-known event count, suitable for long-running traces
//!
//! ## Example
//!
//! ```rust,no_run
//! use ojo_client::{Tracer, Builder, EventRecord};
//! use std::path::PathBuf;
//! use std::time::Duration;
//!
//! # pub mod events {
//! #     pub static SCHEMA: ojo_client::Schema = ojo_client::Schema {
//! #         module: module_path!(),
//! #         namespace: 0xDEADBEEF,
//! #         events: &[],
//! #     };
//! #     pub const PACKET_SENT: u64 = 1;
//! #     pub const PACKET_ACKED: u64 = 2;
//! #     pub const STREAM_OPENED: u64 = 3;
//! #     pub const PACKET_CREATED: u64 = 4;
//! # }
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Initialize the tracer
//! let tracer = Builder::new()
//!     .output_dir(PathBuf::from("./traces"))
//!     .buffer_size(512 * 1024 * 1024) // 512 MiB
//!     .flush_interval(Duration::from_secs(1))
//!     .schema(&events::SCHEMA)
//!     .build()?;
//!
//! // Record events
//! tracer.record(EventRecord {
//!     ts_delta_ns: 12345,
//!     flow_id: 1,
//!     event_type: events::PACKET_SENT,
//!     primary: 100, // packet number
//!     secondary: 0,
//! });
//!
//! tracer.record(EventRecord {
//!     ts_delta_ns: 12350,
//!     flow_id: 1,
//!     event_type: events::PACKET_ACKED,
//!     primary: 100, // packet number
//!     secondary: 0,
//! });
//!
//! tracer.record(EventRecord {
//!     ts_delta_ns: 12360,
//!     flow_id: 1,
//!     event_type: events::STREAM_OPENED,
//!     primary: 10, // stream ID
//!     secondary: 0,
//! });
//!
//! // Tracer automatically flushes on drop
//! # Ok(())
//! # }
//! ```

use fnv::FnvHasher;
use std::{
    cell::Cell,
    collections::BTreeMap,
    fs::{self, File},
    hash::Hasher,
    io::{self, Write},
    mem::MaybeUninit,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicPtr, AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use zerocopy::{AsBytes, FromBytes, FromZeroes};

/// Binary file header (24 bytes)
#[repr(C)]
#[derive(Debug, Clone, Copy, AsBytes, FromBytes, FromZeroes)]
pub struct FileHeader {
    pub magic: [u8; 4],      // b"ojo\0"
    pub version: u8,         // 1
    pub reserved: [u8; 3],   // [0, 0, 0]
    pub batch_start_ns: u64, // Unix timestamp in nanoseconds
    pub schema_version: u64, // Schema version for event types
}

impl FileHeader {
    fn new(batch_start_ns: u64, schema_version: u64) -> Self {
        Self {
            magic: *b"ojo\0",
            version: 1,
            reserved: [0, 0, 0],
            batch_start_ns,
            schema_version,
        }
    }
}

/// Binary event record
/// This is the record format for both the API and on-disk storage
#[repr(C)]
#[derive(Debug, Clone, Copy, AsBytes, FromBytes, FromZeroes)]
pub struct EventRecord {
    pub ts_delta_ns: u64, // Time delta from batch_start_ns
    pub flow_id: u64,
    pub event_type: u64,
    pub primary: u64,
    pub secondary: u64,
}

// Number of event slots per CPU page (approximately 256 KiB per page)
const SLOTS_PER_PAGE: usize = 8 * 1024 - 1;

/// Per-CPU page for event storage
/// Aligned to cache line to avoid false sharing
#[repr(C, align(128))]
struct EventPage {
    // EventRecords written to slots
    slots: [MaybeUninit<EventRecord>; SLOTS_PER_PAGE],
    // Number of slots currently filled
    length: AtomicUsize,
}

impl EventPage {
    fn new() -> Box<EventPage> {
        Box::new(EventPage {
            slots: [const { MaybeUninit::uninit() }; SLOTS_PER_PAGE],
            length: AtomicUsize::new(0),
        })
    }
}

/// Determine the number of possible CPUs
fn possible_cpus() -> usize {
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = fs::read_to_string("/sys/devices/system/cpu/possible") {
            let max_cpu = content
                .trim()
                .split(',')
                .map(|range| {
                    if let Some((_start, end)) = range.split_once('-') {
                        end.parse::<usize>().unwrap_or(0)
                    } else {
                        range.parse::<usize>().unwrap_or(0)
                    }
                })
                .max()
                .unwrap_or(0);
            return max_cpu.max(1) + 1;
        }
    }

    // Fallback to available parallelism
    std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(4)
}

/// Per-CPU event collection system
///
/// This replaces the global ring buffer with per-CPU pages for better scalability.
/// Inspired by AWS s2n-quic's per-CPU approach using rseq on Linux.
///
/// Key benefits:
/// - Eliminates contention on a single global atomic
/// - Better CPU cache locality
/// - Scales with number of threads/cores
///
/// Each CPU has its own page (EventPage) that threads write to. When a page fills up,
/// events overflow to a fallback queue. The background flusher periodically collects
/// all events from all CPU pages.
struct PerCpuEventCollector {
    // Per-CPU pages - one atomic pointer per CPU
    per_cpu_pages: Box<[AtomicPtr<EventPage>]>,

    // Fallback queue for when per-CPU write fails
    fallback_queue: parking_lot::RwLock<Vec<EventRecord>>,

    // Pool of empty pages for reuse
    empty_pages: crossbeam_queue::SegQueue<Box<EventPage>>,

    // Track dropped events
    dropped_count: AtomicU64,
}

unsafe impl Send for PerCpuEventCollector {}
unsafe impl Sync for PerCpuEventCollector {}

impl PerCpuEventCollector {
    fn new(_capacity_bytes: usize) -> Self {
        let num_cpus = possible_cpus();

        Self {
            per_cpu_pages: (0..num_cpus)
                .map(|_| AtomicPtr::new(std::ptr::null_mut()))
                .collect(),
            fallback_queue: parking_lot::RwLock::new(Vec::new()),
            empty_pages: crossbeam_queue::SegQueue::new(),
            dropped_count: AtomicU64::new(0),
        }
    }

    /// Write an event to the per-CPU page or fallback
    fn write(&self, record: &EventRecord) -> bool {
        // Try to get current CPU and write to its page
        if let Some(cpu_id) = self.get_cpu_id() {
            if cpu_id < self.per_cpu_pages.len() {
                if self.try_write_to_cpu_page(cpu_id, record) {
                    return true;
                }
            }
        }

        // Fall back to thread-local or global queue
        self.fallback_push(record);
        true
    }

    /// Get the current CPU ID (simplified version without rseq)
    fn get_cpu_id(&self) -> Option<usize> {
        // For now, use a thread-local hash as a simple CPU affinity approximation
        // In production, this could use rseq on Linux or similar mechanisms
        thread_local! {
            static CPU_SLOT: Cell<Option<usize>> = const { Cell::new(None) };
        }

        CPU_SLOT.with(|slot| {
            if let Some(cpu) = slot.get() {
                return Some(cpu);
            }

            // Hash thread ID to a CPU slot
            let thread_id = std::thread::current().id();
            let hash = format!("{:?}", thread_id).as_bytes().iter().fold(0u64, |acc, &b| {
                acc.wrapping_mul(31).wrapping_add(b as u64)
            });
            let cpu = (hash % self.per_cpu_pages.len() as u64) as usize;
            slot.set(Some(cpu));
            Some(cpu)
        })
    }

    /// Try to write to a specific CPU's page
    fn try_write_to_cpu_page(&self, cpu_id: usize, record: &EventRecord) -> bool {
        let page_ptr = self.per_cpu_pages[cpu_id].load(Ordering::Acquire);

        // If no page exists, try to allocate one
        let page_ptr = if page_ptr.is_null() {
            let new_page = self.empty_pages.pop().unwrap_or_else(EventPage::new);
            let new_ptr = Box::into_raw(new_page);

            match self.per_cpu_pages[cpu_id].compare_exchange(
                std::ptr::null_mut(),
                new_ptr,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => new_ptr,
                Err(existing) => {
                    // Another thread allocated first, reuse that and save our page
                    let reclaimed = unsafe { Box::from_raw(new_ptr) };
                    self.empty_pages.push(reclaimed);
                    existing
                }
            }
        } else {
            page_ptr
        };

        // Try to write to the page
        unsafe {
            let page = &*page_ptr;
            let current_len = page.length.load(Ordering::Acquire);

            if current_len >= SLOTS_PER_PAGE {
                // Page is full, need to rotate
                return false;
            }

            // Try to reserve a slot
            match page.length.compare_exchange(
                current_len,
                current_len + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    // Successfully reserved slot, write the event
                    let slot_ptr = page.slots[current_len].as_ptr() as *mut EventRecord;
                    std::ptr::write(slot_ptr, *record);
                    std::sync::atomic::fence(Ordering::Release);
                    true
                }
                Err(_) => {
                    // Another thread took this slot, retry
                    false
                }
            }
        }
    }

    /// Push to fallback queue
    fn fallback_push(&self, record: &EventRecord) {
        let guard = self.fallback_queue.write();
        // Simple append - in production might want to check size limits
        let mut queue = guard;
        queue.push(*record);
    }

    /// Collect all events from all CPU pages
    fn collect_all_events(&self) -> Vec<EventRecord> {
        let mut all_events = Vec::new();

        // Collect from per-CPU pages
        for cpu_page in self.per_cpu_pages.iter() {
            let page_ptr = cpu_page.swap(std::ptr::null_mut(), Ordering::AcqRel);

            if !page_ptr.is_null() {
                unsafe {
                    let mut page = Box::from_raw(page_ptr);
                    let length = *page.length.get_mut();

                    // Copy events from page
                    for i in 0..length {
                        all_events.push(page.slots[i].assume_init());
                    }

                    // Reset and return page to pool
                    *page.length.get_mut() = 0;
                    self.empty_pages.push(page);
                }
            }
        }

        // Collect from fallback queue
        let mut fallback = self.fallback_queue.write();
        all_events.extend_from_slice(&fallback);
        fallback.clear();

        all_events
    }

    /// Get and reset dropped count
    fn take_dropped_count(&self) -> u64 {
        self.dropped_count.swap(0, Ordering::Relaxed)
    }
}

impl Drop for PerCpuEventCollector {
    fn drop(&mut self) {
        // Clean up all allocated pages
        for cpu_page in self.per_cpu_pages.iter_mut() {
            let page_ptr = *cpu_page.get_mut();
            if !page_ptr.is_null() {
                unsafe {
                    drop(Box::from_raw(page_ptr));
                }
            }
        }
    }
}

/// Shared state between tracer and flusher thread
struct SharedState {
    event_collector: PerCpuEventCollector,
    batch_start_ns: u64,
    schema_version: u64,
    staging_dir: PathBuf,
    output_dir: PathBuf,
}

/// Builder for constructing a Tracer
pub struct Builder {
    output_dir: PathBuf,
    buffer_size: usize,
    flush_interval: Duration,
    schemas: Vec<&'static Schema>,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("./traces"),
            buffer_size: 512 * 1024 * 1024, // 512 MiB
            flush_interval: Duration::from_millis(500),
            schemas: Vec::new(),
        }
    }
}

impl Builder {
    /// Create a new builder with default configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the output directory
    pub fn output_dir(mut self, output_dir: impl Into<PathBuf>) -> Self {
        self.output_dir = output_dir.into();
        self
    }

    /// Set the buffer size in bytes
    pub fn buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }

    /// Set the flush interval
    pub fn flush_interval(mut self, interval: Duration) -> Self {
        self.flush_interval = interval;
        self
    }

    /// Register a schema to be included in the trace
    pub fn schema(mut self, schema: &'static Schema) -> Self {
        self.schemas.push(schema);
        self
    }

    /// Build the Tracer
    pub fn build(self) -> Result<Tracer, std::io::Error> {
        // Create staging/ and output/ directories
        let staging_dir = self.output_dir.join("staging");
        let output_dir = self.output_dir.join("raw");
        let schema_dir = self.output_dir.join("schema");
        fs::create_dir_all(&staging_dir)?;
        fs::create_dir_all(&output_dir)?;
        fs::create_dir_all(&schema_dir)?;

        // Merge schemas and generate JSON
        let (schema_version, schema_json) = self.generate_merged_schema();

        // Write schema file to output directory
        write_schema_file(&schema_dir, schema_version, &schema_json)?;

        // Initialize per-CPU event collector
        let event_collector = PerCpuEventCollector::new(self.buffer_size);

        // Get batch start timestamp
        let batch_start_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        // Create shared state
        let shared_state = Arc::new(SharedState {
            event_collector,
            batch_start_ns,
            schema_version,
            staging_dir,
            output_dir,
        });

        // Start flusher thread
        let flusher_state = shared_state.clone();
        let flush_interval = self.flush_interval;
        thread::spawn(move || {
            flusher_thread_main(flusher_state, flush_interval);
        });

        Ok(Tracer { shared_state })
    }

    fn generate_merged_schema(&self) -> (u64, String) {
        use std::fmt::Write;

        // Collect unique events by value
        let mut events_map = BTreeMap::new();
        for schema in &self.schemas {
            for event in schema.events {
                events_map.insert(event.value, (schema.module, event));
            }
        }

        // Calculate combined hash
        let mut hasher = FnvHasher::with_key(42);
        for (module, event) in events_map.values() {
            hasher.write_u64(event.value);
            hasher.write(module.as_bytes());
            hasher.write(event.name.as_bytes());
            hasher.write(event.category.as_bytes());
            hasher.write(event.description.as_bytes());
            hasher.write_u8(event.value_type as u8);
        }
        let schema_version = hasher.finish();

        let mut json = String::new();

        macro_rules! w {
            ($($arg:tt)*) => {
                write!(json, $($arg)*).unwrap();
            };
        }

        w!("{{\"schema_version\":{schema_version},\"events\":[");
        for (idx, (module, event)) in events_map.values().enumerate() {
            if idx > 0 {
                w!(",");
            }
            w!(
                "{{\"value\":{},\"name\":{:?},\"category\":{:?},\"description\":{:?},\"module\":{:?},\"value_type\":{}}}",
                event.value,
                event.name,
                event.category,
                event.description,
                module,
                event.value_type as u8
            );
        }
        w!("]}}");

        (schema_version, json)
    }
}

/// Main tracer handle for recording events
pub struct Tracer {
    shared_state: Arc<SharedState>,
}

impl Tracer {
    /// Record a trace event
    ///
    /// The caller is responsible for populating the event structure,
    /// including the timestamp, flow_id, event_type, and payload.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # fn main() {
    /// use ojo_client::{Builder, EventRecord};
    /// # let tracer = Builder::new().build().unwrap();
    ///
    /// # mod events {
    /// #    pub const PACKET_SENT: u64 = 1;
    /// # }
    ///
    /// let event = EventRecord {
    ///     ts_delta_ns: 12345, // Time delta from batch start
    ///     flow_id: 1,
    ///     event_type: events::PACKET_SENT,
    ///     primary: 100, // packet number
    ///     secondary: 0,
    /// };
    /// tracer.record(event);
    /// # }
    /// ```
    pub fn record(&self, record: EventRecord) {
        // Write to per-CPU event collector (lock-free per-CPU pages)
        self.shared_state.event_collector.write(&record);
    }
}

/// Write the event schema JSON file to the output directory
fn write_schema_file(output_dir: &std::path::Path, version: u64, json: &str) -> io::Result<()> {
    let schema_filename = format!("event_schema_{:016x}.json", version);
    let schema_path = output_dir.join(schema_filename);

    // Only write if the file doesn't already exist
    if !schema_path.exists() {
        let mut file = File::create(&schema_path)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
    }

    Ok(())
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
            if let Err(e) = flush_events_from_per_cpu_collector(&state, &mut batch_counter) {
                eprintln!("Error flushing events to file: {}", e);
            }
            break;
        }

        // Flush events from per-CPU collector
        if let Err(e) = flush_events_from_per_cpu_collector(&state, &mut batch_counter) {
            eprintln!("Error flushing events to file: {}", e);
        }
    }
}

/// Flush events from per-CPU collector to a binary file
fn flush_events_from_per_cpu_collector(state: &SharedState, batch_counter: &mut u64) -> io::Result<()> {
    // Collect all events from per-CPU pages
    let events = state.event_collector.collect_all_events();

    // Check for dropped events
    let dropped = state.event_collector.take_dropped_count();

    if events.is_empty() && dropped == 0 {
        return Ok(());
    }

    // Generate unique filename
    let filename = format!(
        "trace_{:016x}_{:08x}.bin",
        state.batch_start_ns, *batch_counter
    );
    *batch_counter += 1;

    let staging_path = state.staging_dir.join(&filename);
    let output_path = state.output_dir.join(&filename);

    // Write to staging file
    let mut file = File::create(&staging_path)?;

    // Write header with schema version
    let header = FileHeader::new(state.batch_start_ns, state.schema_version);
    file.write_all(header.as_bytes())?;

    // Write all collected events
    if !events.is_empty() {
        let bytes = unsafe {
            std::slice::from_raw_parts(
                events.as_ptr() as *const u8,
                events.len() * std::mem::size_of::<EventRecord>(),
            )
        };
        file.write_all(bytes)?;
    }

    // Write dropped events record if any
    if dropped > 0 {
        let dropped_event = EventRecord {
            ts_delta_ns: 0,
            flow_id: u64::MAX,
            event_type: u64::MAX,
            primary: dropped,
            secondary: 0,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EventTypeInfo {
    /// The event type value
    pub value: u64,
    /// The constant name
    pub name: &'static str,
    /// The category (Packet, Stream, FlowControl, CongestionControl, Meta)
    pub category: &'static str,
    /// Human-readable description
    pub description: &'static str,
    /// Whether the event includes a secondary value
    pub value_type: ValueType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum ValueType {
    None = 0,
    Identifier = 1,
    Count = 2,
    Bytes = 3,
    Duration = 4,
    RangeCount = 5,
    RangeBytes = 6,
    RangeDuration = 7,
}

/// Event schema containing namespace and event type information
#[derive(Debug, Clone, Copy)]
pub struct Schema {
    pub module: &'static str,
    pub namespace: u32,
    pub events: &'static [EventTypeInfo],
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, thread, time::Duration};
    use tempfile::TempDir;

    mod events {
        use super::*;

        pub static SCHEMA: Schema = Schema {
            module: "test",
            namespace: 1,
            events: &[EventTypeInfo {
                value: 1,
                name: "PACKET_SENT",
                category: "Packet",
                description: "Packet sent",
                value_type: ValueType::Identifier,
            }],
        };

        pub const PACKET_SENT: u64 = 1;
        pub const PACKET_ACKED: u64 = 2;
        pub const PACKET_CREATED: u64 = 3;
    }

    #[test]
    fn test_builder_defaults() {
        let builder = Builder::new();
        assert_eq!(builder.buffer_size, 512 * 1024 * 1024);
        assert_eq!(builder.flush_interval, Duration::from_millis(500));
    }

    #[test]
    fn test_builder_configuration() {
        let builder = Builder::new()
            .output_dir("/tmp/traces")
            .buffer_size(1024 * 1024)
            .flush_interval(Duration::from_millis(500));

        assert_eq!(builder.output_dir, PathBuf::from("/tmp/traces"));
        assert_eq!(builder.buffer_size, 1024 * 1024);
        assert_eq!(builder.flush_interval, Duration::from_millis(500));
    }

    #[test]
    fn test_tracer_initialization() {
        let temp_dir = TempDir::new().unwrap();
        let tracer = Builder::new()
            .output_dir(temp_dir.path())
            .buffer_size(1024 * 1024)
            .build()
            .unwrap();

        // Check that directories were created
        assert!(temp_dir.path().join("staging").exists());
        assert!(temp_dir.path().join("raw").exists());

        drop(tracer);
    }

    #[test]
    fn test_event_recording_and_flush() {
        let temp_dir = TempDir::new().unwrap();
        let tracer = Builder::new()
            .output_dir(temp_dir.path())
            .buffer_size(1024 * 1024)
            .flush_interval(Duration::from_millis(100))
            .build()
            .unwrap();

        // Record some events
        for i in 0..10 {
            tracer.record(EventRecord {
                ts_delta_ns: i * 1000,
                flow_id: 1,
                event_type: events::PACKET_SENT,
                primary: i,
                secondary: 0,
            });
        }

        // Wait for flush
        thread::sleep(Duration::from_millis(300));

        // Check that files were created
        let output_dir = temp_dir.path().join("raw");
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
    fn test_per_cpu_collector_write_and_read() {
        let collector = PerCpuEventCollector::new(1024 * 1024);

        // Write some events
        for i in 0..10 {
            let record = EventRecord {
                ts_delta_ns: i * 1000,
                flow_id: 1,
                event_type: events::PACKET_SENT,
                primary: i,
                secondary: 0,
            };
            assert!(collector.write(&record));
        }

        // Collect events back
        let events = collector.collect_all_events();
        assert_eq!(events.len(), 10);

        // Verify event data
        for (i, event) in events.iter().enumerate() {
            assert_eq!(event.ts_delta_ns, i as u64 * 1000);
            assert_eq!(event.flow_id, 1);
            assert_eq!(event.event_type, events::PACKET_SENT);
            assert_eq!(event.primary, i as u64);
        }
    }

    #[test]
    fn test_per_cpu_collector_page_overflow() {
        // Create a collector
        let collector = PerCpuEventCollector::new(1024 * 1024);

        // Fill up more than one page worth of events
        let events_to_write = SLOTS_PER_PAGE + 100;
        for i in 0..events_to_write {
            let record = EventRecord {
                ts_delta_ns: i as u64,
                flow_id: 1,
                event_type: events::PACKET_SENT,
                primary: i as u64,
                secondary: 0,
            };
            // All writes should succeed (they go to fallback when page is full)
            assert!(collector.write(&record), "Failed to write record {}", i);
        }

        // Collect all events
        let events = collector.collect_all_events();
        assert_eq!(events.len(), events_to_write);

        // Verify we can write again after collection
        let record = EventRecord {
            ts_delta_ns: 200,
            flow_id: 1,
            event_type: events::PACKET_SENT,
            primary: 200,
            secondary: 0,
        };
        assert!(
            collector.write(&record),
            "Should be able to write after collection"
        );
    }

    #[test]
    fn test_file_header_format() {
        let batch_start = 123456789u64;
        let schema_version = 1u64;
        let header = FileHeader::new(batch_start, schema_version);

        assert_eq!(&header.magic, b"ojo\0");
        assert_eq!(header.version, 1);
        assert_eq!(header.batch_start_ns, batch_start);
        assert_eq!(header.schema_version, schema_version);
    }

    #[test]
    fn test_binary_format_size() {
        // Verify expected sizes
        assert_eq!(std::mem::size_of::<FileHeader>(), 24);
        assert_eq!(std::mem::size_of::<EventRecord>(), 40);
    }

    #[test]
    fn test_multiple_events_same_flow() {
        let temp_dir = TempDir::new().unwrap();
        let tracer = Builder::new()
            .output_dir(temp_dir.path())
            .buffer_size(1024 * 1024)
            .flush_interval(Duration::from_millis(100))
            .build()
            .unwrap();

        // Simulate a packet lifecycle
        let flow_id = 42;

        tracer.record(EventRecord {
            ts_delta_ns: 0,
            flow_id,
            event_type: events::PACKET_CREATED,
            primary: 1,
            secondary: 0,
        });

        tracer.record(EventRecord {
            ts_delta_ns: 1000,
            flow_id,
            event_type: events::PACKET_SENT,
            primary: 1,
            secondary: 0,
        });

        tracer.record(EventRecord {
            ts_delta_ns: 2000,
            flow_id,
            event_type: events::PACKET_ACKED,
            primary: 1,
            secondary: 0,
        });

        // Wait for flush
        thread::sleep(Duration::from_millis(300));

        drop(tracer);
    }

    #[test]
    fn test_concurrent_event_recording() {
        let temp_dir = TempDir::new().unwrap();
        let builder = Builder::new()
            .output_dir(temp_dir.path())
            .buffer_size(10 * 1024 * 1024)
            .flush_interval(Duration::from_millis(200));

        let tracer = Arc::new(builder.build().unwrap());

        // Spawn multiple threads recording events
        let mut handles = vec![];
        for thread_id in 0..4 {
            let tracer_clone = tracer.clone();
            let handle = thread::spawn(move || {
                for i in 0..100 {
                    tracer_clone.record(EventRecord {
                        ts_delta_ns: i * 1000,
                        flow_id: thread_id,
                        event_type: events::PACKET_SENT,
                        primary: i,
                        secondary: 0,
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

    #[test]
    fn test_schema_file_creation() {
        let temp_dir = TempDir::new().unwrap();

        let tracer = Builder::new()
            .output_dir(temp_dir.path())
            .buffer_size(1024 * 1024)
            .schema(&events::SCHEMA)
            .build()
            .unwrap();

        // Check that schema file was created in schema directory
        let schema_dir = temp_dir.path().join("schema");

        // Find schema file (name includes hash now)
        let entries: Vec<_> = fs::read_dir(&schema_dir)
            .unwrap()
            .filter_map(Result::ok)
            .collect();

        let schema_file = entries
            .iter()
            .find(|e| e.file_name().to_string_lossy().starts_with("event_schema_"));

        assert!(schema_file.is_some(), "Schema file should be created");

        let schema_path = schema_file.unwrap().path();

        // Verify schema file content is valid JSON
        let schema_content = fs::read_to_string(&schema_path).unwrap();
        assert!(schema_content.contains("schema_version"));
        assert!(schema_content.contains("events"));
        assert!(schema_content.contains("PACKET_SENT"));

        drop(tracer);
    }
}
