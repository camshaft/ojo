# Ojo Project Plan

**Project**: Transport Protocol Event Tracing System  
**Version**: 1.0  
**Date**: January 2026

## 1. Project Overview

Ojo is a low-overhead transport protocol event tracing system designed for debugging and analyzing network protocol behavior. The system consists of three main components:

1. **Client Library** (`ojo-client`) - Rust library for collecting and writing events to binary files
2. **Watcher Process** (`ojo-watcher`) - Background service that monitors directories and transforms data into a queryable format
3. **Browser Explorer** (`ojo-explorer`) - Web-based interface for querying and visualizing trace data

## 2. Design Reference

The implementation follows the Transport Protocol Event Tracing Format specification:
https://gist.githubusercontent.com/camshaft/fb54b7f99677e170d5ee1588744ce339/raw/README.md

### Key Design Principles

- **Lock-free multi-writer**: Minimal overhead for event recording
- **Fixed-size records**: 24-byte events for predictable layout and zero-copy parsing
- **Streaming format**: No pre-known event count, suitable for long-running traces
- **Atomic batch visibility**: Directory watcher pattern for safe concurrent access
- **Queryable output**: Transform binary traces into SQLite or Apache Arrow format

## 3. Repository Structure

```
ojo/
├── Cargo.toml              # Workspace manifest
├── PROJECT_PLAN.md         # This document
├── README.md               # User-facing documentation
├── LICENSE                 # MIT or Apache-2.0
├── .gitignore             
│
├── crates/
│   ├── ojo-client/         # Client library
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs      # Public API
│   │   │   ├── tracer.rs   # Core tracer implementation
│   │   │   ├── buffer.rs   # Lock-free ring buffer
│   │   │   ├── flusher.rs  # Background flusher thread
│   │   │   ├── event.rs    # Event type definitions
│   │   │   └── format.rs   # Binary format (header, records)
│   │   ├── benches/        # Performance benchmarks
│   │   └── examples/       # Usage examples
│   │
│   ├── ojo-watcher/        # Watcher daemon
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── main.rs     # CLI entry point
│   │   │   ├── lib.rs      # Library for programmatic use
│   │   │   ├── watcher.rs  # Directory monitoring
│   │   │   ├── parser.rs   # Binary format parser
│   │   │   ├── transform.rs # Data transformation logic
│   │   │   ├── sqlite.rs   # SQLite backend
│   │   │   └── arrow.rs    # Apache Arrow backend (future)
│   │   └── examples/
│   │
│   └── ojo-explorer/       # Browser-based UI
│       ├── Cargo.toml
│       ├── src/
│       │   ├── main.rs     # Web server (Axum/Actix)
│       │   ├── api.rs      # REST API endpoints
│       │   └── query.rs    # Query engine
│       └── web/            # Frontend assets
│           ├── index.html
│           ├── app.js      # Vanilla JS or lightweight framework
│           └── style.css
│
├── docs/
│   ├── ARCHITECTURE.md     # System architecture
│   ├── API.md             # Client library API reference
│   └── QUERY.md           # Query language documentation
│
└── examples/
    ├── simple/            # Basic usage example
    ├── multi_flow/        # Multiple concurrent flows
    └── performance/       # Performance testing
```

## 4. Component Details

### 4.1 ojo-client (Client Library)

**Purpose**: Low-overhead event collection and binary file writing

**Key Features**:
- Lock-free ring buffer with atomic operations
- Background flusher thread
- Zero-allocation event recording path
- Support for all event types from specification
- Configurable buffer size and flush intervals
- Thread-safe multi-writer, single-reader architecture

**API Design**:
```rust
// Core API
pub struct Tracer { /* ... */ }
impl Tracer {
    pub fn new(config: TracerConfig) -> Result<Self, Error>;
    pub fn record_packet_sent(&self, flow_id: u32, packet_number: u64);
    pub fn record_packet_acked(&self, flow_id: u32, packet_number: u64);
    pub fn record_stream_opened(&self, flow_id: u32, stream_id: u64);
    // ... other event recording methods
}

pub struct TracerConfig {
    pub output_dir: PathBuf,
    pub buffer_size: usize,      // e.g., 512 MiB
    pub flush_interval: Duration, // e.g., 1-5 seconds
}
```

**Dependencies**:
- `zerocopy` - Zero-copy serialization
- `parking_lot` - Efficient synchronization primitives
- No allocations on hot path

**Testing Strategy**:
- Unit tests for buffer operations
- Integration tests for concurrent writers
- Benchmarks for throughput and latency
- Stress tests for buffer overflow scenarios

### 4.2 ojo-watcher (Watcher Process)

**Purpose**: Monitor trace directories and transform binary data into queryable format

**Key Features**:
- Directory monitoring with `notify` crate
- Parse binary trace files
- Transform to SQLite schema (initial implementation)
- Future: Apache Arrow/Parquet support
- Handle file rotation and cleanup
- Detect and log dropped events

**SQLite Schema Design**:
```sql
CREATE TABLE events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_start_us INTEGER NOT NULL,
    ts_delta_us INTEGER NOT NULL,
    flow_id INTEGER NOT NULL,
    event_type INTEGER NOT NULL,
    payload INTEGER NOT NULL,
    file_name TEXT NOT NULL
);

CREATE INDEX idx_flow_id ON events(flow_id);
CREATE INDEX idx_event_type ON events(event_type);
CREATE INDEX idx_timestamp ON events(ts_delta_us);

CREATE TABLE event_types (
    event_type INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    category TEXT NOT NULL,
    description TEXT
);

CREATE TABLE flows (
    flow_id INTEGER PRIMARY KEY,
    parent_flow_id INTEGER,
    first_event_ts INTEGER,
    last_event_ts INTEGER
);
```

**CLI Interface**:
```bash
ojo-watcher --input-dir ./output \
            --db-path ./traces.db \
            --watch \
            --cleanup-age 7d
```

**Dependencies**:
- `notify` - File system monitoring
- `rusqlite` - SQLite database
- `clap` - CLI argument parsing
- `zerocopy` - Binary parsing

### 4.3 ojo-explorer (Browser Interface)

**Purpose**: Web-based query and visualization interface

**Key Features**:
- REST API for querying traces
- Filter by flow_id, event_type, time range
- Visualizations:
  - Packet progression graphs
  - Stream lifecycle timelines
  - Congestion window evolution
  - Flow control updates
- Export capabilities (JSON, CSV)
- Real-time updates when watcher is running

**Technology Stack**:
- Backend: Rust with Axum web framework
- Frontend: Vanilla JavaScript or lightweight framework (Svelte/Alpine.js)
- Charting: D3.js or Chart.js
- No complex build pipeline initially

**API Endpoints**:
```
GET  /api/flows                    # List all flows
GET  /api/flows/:id/events         # Get events for flow
GET  /api/events?type=...&from=... # Query events
GET  /api/stats                    # Summary statistics
```

## 5. Implementation Phases

### Phase 1: Foundation (Week 1-2)
- [x] Set up Cargo workspace
- [ ] Implement binary format structures (`format.rs`)
- [ ] Implement lock-free ring buffer (`buffer.rs`)
- [ ] Basic tracer with single event type
- [ ] Unit tests for core components

### Phase 2: Client Library (Week 2-3)
- [ ] Complete event type implementations
- [ ] Background flusher thread
- [ ] File naming and atomic moves (staging → output)
- [ ] Multi-threaded stress tests
- [ ] Performance benchmarks
- [ ] Documentation and examples

### Phase 3: Watcher (Week 3-4)
- [ ] Binary file parser
- [ ] SQLite schema and writer
- [ ] Directory monitoring
- [ ] Event type metadata
- [ ] Flow hierarchy tracking
- [ ] CLI interface and configuration

### Phase 4: Explorer (Week 4-5)
- [ ] REST API implementation
- [ ] Basic web UI
- [ ] Query interface
- [ ] Simple visualizations (timelines)
- [ ] Real-time updates

### Phase 5: Polish & Extensions (Week 5-6)
- [ ] Advanced visualizations
- [ ] Export functionality
- [ ] Apache Arrow backend investigation
- [ ] Performance optimization
- [ ] Comprehensive documentation
- [ ] Tutorial and examples

## 6. Design Decisions

### 6.1 Storage Format (Watcher Output)

**Initial Choice: SQLite**
- Pros: Simple, embedded, good query performance, widely supported
- Cons: Single writer limitation (but watcher is single writer)

**Future: Apache Arrow/Parquet**
- Pros: Columnar format, excellent for analytics, language-agnostic
- Cons: More complex, requires additional dependencies
- Decision: Implement as alternative backend in Phase 5

### 6.2 Buffer Parameters

**Recommended Defaults**:
- Buffer size: 512 MiB (can hold ~22M events before wrap)
- Flush interval: 1 second (balance between latency and I/O)
- Flush threshold: 10 MiB or 1 second, whichever comes first

### 6.3 File Naming

**Pattern**: `trace_{monotonic_us}_{sequence}.bin`
- `monotonic_us`: Timestamp from monotonic clock
- `sequence`: Atomic counter per process (prevents collisions)

### 6.4 Event Type Constants

Define in `event.rs` with clear documentation:
```rust
pub mod event_type {
    pub const PACKET_CREATED: u32 = 0x00000001;
    pub const PACKET_SENT: u32 = 0x00000002;
    pub const PACKET_ACKED: u32 = 0x00000003;
    // ... etc
}
```

## 7. Testing Strategy

### 7.1 Unit Tests
- Lock-free buffer operations
- Binary format serialization/deserialization
- Event type validation
- File naming and sequencing

### 7.2 Integration Tests
- Multi-threaded writer scenarios
- File watcher detection and parsing
- End-to-end: write → watch → query
- Overflow and dropped event handling

### 7.3 Performance Tests
- Benchmark: events/second throughput
- Benchmark: nanoseconds per event (hot path)
- Stress: sustained high load (hours)
- Stress: buffer overflow scenarios

### 7.4 Correctness Tests
- Verify no events lost (when buffer adequate)
- Verify proper dropped event accounting
- Verify atomic batch visibility
- Verify flow hierarchy reconstruction

## 8. Documentation Plan

### 8.1 User Documentation
- README.md: Quick start and overview
- Getting started guide
- API reference (rustdoc)
- Configuration guide
- Troubleshooting guide

### 8.2 Developer Documentation
- Architecture document
- Binary format specification
- Contributing guide
- Design rationale

### 8.3 Examples
- Simple single-flow tracing
- Multi-flow concurrent tracing
- Custom event types
- Integration with existing protocol implementations

## 9. Deployment & Distribution

### 9.1 Package Distribution
- Publish `ojo-client` to crates.io
- Distribute `ojo-watcher` and `ojo-explorer` as binaries
- Docker images for watcher and explorer
- Homebrew formula (future)

### 9.2 Platform Support
- Primary: Linux (x86_64, aarch64)
- Secondary: macOS (x86_64, aarch64)
- Future: Windows

## 10. Success Criteria

### Performance
- ✓ < 100 ns per event on hot path
- ✓ Zero allocations on event recording path
- ✓ Support 1M+ events/second with 4 concurrent writers

### Reliability
- ✓ No data loss when buffer sized appropriately
- ✓ Accurate dropped event accounting
- ✓ Atomic batch visibility (no partial reads)

### Usability
- ✓ Simple API (< 10 LOC to start tracing)
- ✓ Clear documentation with examples
- ✓ Intuitive query interface in explorer

## 11. Future Extensions

### Short-term (3-6 months)
- Apache Arrow/Parquet backend
- Advanced visualizations (packet flow diagrams)
- Live streaming mode (bypass files)
- Filtering at collection time

### Long-term (6-12 months)
- Language bindings (C, Go, Python)
- Distributed tracing (multiple processes)
- Trace comparison tools
- Machine learning integration for anomaly detection

## 12. Open Questions

1. **Compression**: Should we compress batch files after writing?
   - Recommendation: Optional post-processing step, not in critical path

2. **Sequence numbers**: Should events have global sequence numbers?
   - Recommendation: Not needed; timestamp + flow_id sufficient for ordering

3. **Schema evolution**: How to handle format version changes?
   - Recommendation: Version in header, watcher checks and handles appropriately

4. **Retention policy**: How long to keep raw binary files?
   - Recommendation: Configurable in watcher (e.g., delete after 7 days)

## 13. Timeline Summary

- **Week 1-2**: Foundation and binary format
- **Week 2-3**: Complete client library
- **Week 3-4**: Watcher implementation
- **Week 4-5**: Browser explorer
- **Week 5-6**: Polish and documentation

**Target for MVP**: 4 weeks  
**Target for v1.0**: 6 weeks

## 14. Resources

- Design Specification: https://gist.githubusercontent.com/camshaft/fb54b7f99677e170d5ee1588744ce339/raw/README.md
- Repository: https://github.com/camshaft/ojo
- Similar projects (reference): Tokio Console, Firefox Profiler, Chrome Tracing

---

**Document Status**: Living document, updated as implementation progresses  
**Last Updated**: January 22, 2026  
**Maintainer**: @camshaft
