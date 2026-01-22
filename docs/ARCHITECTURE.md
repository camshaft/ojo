# Ojo Architecture

## System Overview

Ojo is a transport protocol event tracing system consisting of three main components that work together to provide low-overhead event collection, transformation, and visualization.

```
┌─────────────────────────────────────────────────────────────────┐
│                        User Application                          │
│                                                                   │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │                      ojo-client                            │ │
│  │  ┌──────────┐     ┌──────────┐     ┌────────────────┐    │ │
│  │  │  Tracer  │────▶│  Buffer  │────▶│ Flusher Thread │    │ │
│  │  │   API    │     │(Lock-free)│     │  (Background)  │    │ │
│  │  └──────────┘     └──────────┘     └────────┬───────┘    │ │
│  └─────────────────────────────────────────────┼────────────┘ │
└────────────────────────────────────────────────┼──────────────┘
                                                  │
                                      Binary Files (.bin)
                           staging/ ──atomic move──▶ output/
                                                  │
┌─────────────────────────────────────────────────┼──────────────┐
│                      ojo-watcher                │               │
│  ┌───────────────┐     ┌────────────┐     ┌────▼──────────┐   │
│  │ File System   │────▶│   Parser   │────▶│ SQLite Writer │   │
│  │   Watcher     │     │ (Binary)   │     │               │   │
│  └───────────────┘     └────────────┘     └───────────────┘   │
│                                                  │               │
└──────────────────────────────────────────────────┼──────────────┘
                                                    │
                                            SQLite Database
                                               traces.db
                                                    │
┌───────────────────────────────────────────────────┼─────────────┐
│                     ojo-explorer                  │              │
│  ┌───────────┐     ┌──────────┐     ┌────────────▼─────────┐   │
│  │    Web    │────▶│   API    │────▶│   Query Engine      │   │
│  │ Interface │◀────│ (Axum)   │◀────│   (SQLite)          │   │
│  └───────────┘     └──────────┘     └──────────────────────┘   │
│   Browser                                                        │
└──────────────────────────────────────────────────────────────────┘
```

## Component Details

### 1. ojo-client (Client Library)

**Purpose**: Provide a high-performance, low-overhead event recording API.

**Key Components**:

#### Tracer
- Public API for recording events
- Configuration management
- Lifecycle management (init/shutdown)

#### Lock-free Ring Buffer
- Fixed-size circular buffer (default: 512 MiB)
- Atomic operations for multi-writer access
- Zero-allocation on hot path
- `Arc<Vec<u8>>` for shared memory
- `AtomicUsize` for write_head and read_head pointers
- `AtomicU64` for dropped event counter

#### Background Flusher Thread
- Single reader for the ring buffer
- Periodic flushes (configurable interval)
- Writes to staging directory
- Atomic moves to output directory
- Handles dropped event recording

**Data Flow**:
1. Application calls `tracer.record_packet_sent(flow_id, packet_num)`
2. Event struct populated with timestamp and data
3. Atomic CAS loop to reserve buffer space
4. Memory copy to buffer (handles wrap-around)
5. Release fence for visibility
6. Flusher thread periodically reads buffer
7. Writes batch to staging file
8. Atomic rename to output directory

**Performance Characteristics**:
- < 100 ns per event (hot path)
- Zero allocations during recording
- Lock-free writes (only atomic operations)
- 1M+ events/second with 4 concurrent writers

### 2. ojo-watcher (Watcher Daemon)

**Purpose**: Transform binary trace files into a queryable database format.

**Key Components**:

#### File System Watcher
- Uses `notify` crate for efficient file watching
- Monitors output directory for new `.bin` files
- Debouncing to handle file completion

#### Binary Parser
- Zero-copy deserialization with `zerocopy`
- Validates 24-byte header (magic, version)
- Reads 24-byte event records
- Detects and logs dropped events

#### SQLite Writer
- Inserts events into normalized schema
- Builds flow hierarchy from STREAM_LINK_PARENT events
- Creates indexes for efficient queries
- Transaction batching for performance

#### Transformation Pipeline
```
.bin file → Validate Header → Parse Events → Transform → Insert → Index
```

**Database Schema**:

```sql
-- Main events table
CREATE TABLE events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_start_us INTEGER NOT NULL,
    ts_delta_us INTEGER NOT NULL,
    flow_id INTEGER NOT NULL,
    event_type INTEGER NOT NULL,
    payload INTEGER NOT NULL,
    file_name TEXT NOT NULL
);

-- Event type metadata
CREATE TABLE event_types (
    event_type INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    category TEXT NOT NULL,
    description TEXT
);

-- Flow hierarchy
CREATE TABLE flows (
    flow_id INTEGER PRIMARY KEY,
    parent_flow_id INTEGER,
    first_event_ts INTEGER,
    last_event_ts INTEGER,
    event_count INTEGER DEFAULT 0
);

-- Indexes
CREATE INDEX idx_flow_id ON events(flow_id);
CREATE INDEX idx_event_type ON events(event_type);
CREATE INDEX idx_timestamp ON events(ts_delta_us);
```

### 3. ojo-explorer (Web Interface)

**Purpose**: Provide an interactive browser-based interface for querying and visualizing traces.

**Key Components**:

#### REST API (Axum)
- `/api/flows` - List all flows with metadata
- `/api/flows/:id/events` - Get events for specific flow
- `/api/events` - Query events with filters
- `/api/stats` - Summary statistics
- `/api/export` - Export data (JSON, CSV)

#### Query Engine
- SQL query builder
- Filter by flow_id, event_type, time range
- Pagination support
- Aggregation queries (stats, counts)

#### Web Frontend
- Vanilla JavaScript (no complex build)
- D3.js for visualizations
- Real-time updates via polling or WebSocket
- Responsive design

**Visualizations**:
1. **Packet Timeline** - Linear view of packet events
2. **Stream Lifecycle** - Gantt-style stream durations
3. **Congestion Window** - Line chart of CWND over time
4. **Flow Control** - Max data progression
5. **Event Distribution** - Histogram of event types

## Binary Format Specification

### File Header (24 bytes)
```
Offset | Size | Field           | Value/Type
-------+------+-----------------+-----------
0      | 4    | magic           | b"TRAC"
4      | 1    | version         | 1
5      | 3    | reserved        | [0,0,0]
8      | 8    | batch_start_us  | u64
16     | 8    | reserved        | 0
```

### Event Record (24 bytes)
```
Offset | Size | Field           | Type
-------+------+-----------------+------
0      | 8    | ts_delta_us     | u64
8      | 4    | flow_id         | u32
12     | 4    | event_type      | u32
16     | 8    | payload         | u64
```

**Key Properties**:
- Fixed size for predictable layout
- Little-endian encoding
- Aligned to 8-byte boundaries
- No padding required
- Zero-copy deserialization compatible

## Concurrency Model

### ojo-client

**Multi-writer, Single-reader**:
- Multiple threads can record events concurrently
- Lock-free CAS operations for buffer space reservation
- Single background thread for flushing
- Memory fences ensure visibility across threads

**Synchronization Points**:
1. Buffer space reservation (CAS on write_head)
2. Visibility fence after write
3. Flush notification (condvar)
4. Read head update after flush

### ojo-watcher

**Single-threaded processing**:
- One file at a time (no concurrent file processing)
- SQLite writer in WAL mode for better concurrency
- Can process files while client is writing new ones

### ojo-explorer

**Request-per-query**:
- Each HTTP request is independent
- SQLite connections from pool
- Read-only queries (no write contention)

## Performance Characteristics

### Throughput
- **Client**: 1M+ events/second (4 writers)
- **Watcher**: 500K+ events/second parsing
- **Explorer**: Sub-millisecond query latency

### Latency
- **Event recording**: < 100 ns (p99)
- **Flush to disk**: 1-5 seconds (configurable)
- **Query response**: < 10 ms (typical)

### Memory
- **Client buffer**: 512 MiB (configurable)
- **Watcher**: < 100 MiB resident
- **Explorer**: < 50 MiB + query cache

### Disk
- **Binary files**: ~24 bytes per event + 24 byte header
- **SQLite**: ~40-50 bytes per event (with indexes)
- **Compression**: ~5-10x with gzip (post-processing)

## Failure Modes and Recovery

### Buffer Overflow (Client)
- When buffer is full, increment dropped_count
- Continue attempting to write (backoff)
- Flusher records EVENTS_DROPPED event

### File System Issues
- **No space**: Flusher logs error, continues
- **Permission denied**: Fatal error on init
- **Staging rename fails**: Retry with backoff

### Database Issues (Watcher)
- **Corrupt file**: Log warning, skip file
- **Insert fails**: Transaction rollback, retry
- **Disk full**: Stop processing, log error

### Web Server Issues (Explorer)
- **DB locked**: Retry with timeout
- **Query timeout**: Return partial results
- **Memory limit**: Implement result pagination

## Extension Points

### Custom Event Types
- Define new event_type constants
- Update event_types table
- No code changes required in client

### Alternative Storage Backends
- Apache Arrow/Parquet
- InfluxDB
- Prometheus metrics

### Custom Visualizations
- Plugin architecture for new chart types
- Export API for external tools
- Integration with Grafana

## Security Considerations

### Client
- No network exposure
- File system permissions for trace directory
- No sensitive data in traces (user responsibility)

### Watcher
- Local file access only
- SQLite injection prevention (parameterized queries)
- File validation before parsing

### Explorer
- Read-only database access
- CORS configuration for API
- No authentication (local use assumed)
- For production: add auth, rate limiting, TLS

## Deployment Patterns

### Development
```
ojo-client (embedded in app)
  ↓
local file system
  ↓
ojo-watcher --watch (terminal)
  ↓
ojo-explorer (terminal)
  ↓
http://localhost:8080 (browser)
```

### Production
```
ojo-client (embedded)
  ↓
shared/network file system
  ↓
ojo-watcher (systemd service)
  ↓
ojo-explorer (systemd service) + nginx
  ↓
https://traces.example.com (browser)
```

### Distributed
```
Multiple clients on different hosts
  ↓
Central NFS/S3 mount
  ↓
Multiple watchers (partitioned by file)
  ↓
Single SQLite or migrate to ClickHouse
  ↓
ojo-explorer cluster
```

## Future Enhancements

### Short-term
- Apache Arrow backend for analytical queries
- Real-time streaming (bypass files)
- Advanced visualizations
- Filtering at collection time

### Long-term
- Distributed tracing across processes
- Language bindings (C, Go, Python)
- Machine learning anomaly detection
- Trace comparison and diff tools
- Integration with observability platforms

---

**Last Updated**: January 22, 2026
