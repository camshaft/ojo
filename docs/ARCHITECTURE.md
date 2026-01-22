# Ojo Architecture

## System Overview

Ojo is a transport protocol event tracing system consisting of three main components that work together to provide low-overhead event collection, transformation, and visualization.

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
1. Application calls event recording method
2. Event struct populated with timestamp and data
3. Atomic CAS loop to reserve buffer space
4. Memory copy to buffer (handles wrap-around)
5. Release fence for visibility
6. Flusher thread periodically reads buffer
7. Writes batch to staging file
8. Atomic rename to output directory

### 2. ojo (CLI Tool)

**Purpose**: Transform binary trace files into a queryable database format and serve web interface.

**Key Components**:

#### File System Watcher (ojo watch)
- Monitors output directory for new `.bin` files
- Debouncing to handle file completion

#### Binary Parser
- Zero-copy deserialization with `zerocopy`
- Validates 24-byte header (magic, version)
- Reads 32-byte event records
- Detects and logs dropped events

#### Database Writer
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
-- Batches table
CREATE TABLE batches (
    batch_id INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_start_ns INTEGER NOT NULL,
    file_name TEXT NOT NULL
);

-- Main events table
CREATE TABLE events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_id INTEGER NOT NULL,
    timestamp_ns INTEGER NOT NULL,  -- Absolute timestamp (batch_start_ns + ts_delta_ns)
    flow_id INTEGER NOT NULL,
    event_type INTEGER NOT NULL,
    payload INTEGER NOT NULL,
    FOREIGN KEY (batch_id) REFERENCES batches(batch_id)
);

-- Flow hierarchy (flow_id qualified by batch_id)
CREATE TABLE flows (
    batch_id INTEGER NOT NULL,
    flow_id INTEGER NOT NULL,
    parent_flow_id INTEGER,
    first_event_ts INTEGER,
    last_event_ts INTEGER,
    event_count INTEGER DEFAULT 0,
    PRIMARY KEY (batch_id, flow_id)
);

-- Indexes
CREATE INDEX idx_batch_id ON events(batch_id);
CREATE INDEX idx_flow_id ON events(batch_id, flow_id);
CREATE INDEX idx_event_type ON events(event_type);
CREATE INDEX idx_timestamp ON events(timestamp_ns);
```

#### Web Server (ojo serve)

**Purpose**: Provide an interactive browser-based interface for querying and visualizing traces.

**Key Components**:

#### REST API (Axum)
Provides endpoints for querying trace data and retrieving flow information.

#### Query Engine
- SQL query builder
- Filter by flow_id, event_type, time range
- Pagination support
- Aggregation queries (stats, counts)

#### Web Frontend
- TypeScript with Vite build system
- Tailwind CSS for styling
- Observable Plot for visualizations
- Real-time updates via polling or WebSocket
- Responsive design

**Visualizations**:
1. **Offset Timeline** - Overlay of offsets being transmitted, acked, retransmitted for a given flow
2. **Stream Lifecycle** - Stream durations and relationships
3. **Congestion Window** - CWND evolution over time
4. **Flow Control** - Max data progression
5. **Event Distribution** - Event type histograms

## Binary Format Specification

### File Header (24 bytes)
```
Offset | Size | Field           | Value/Type
-------+------+-----------------+-----------
0      | 4    | magic           | b"ojo\0"
4      | 1    | version         | 1
5      | 3    | reserved        | [0,0,0]
8      | 8    | batch_start_ns  | u64
16     | 8    | reserved        | 0
```

### Event Record (32 bytes)
```
Offset | Size | Field           | Type
-------+------+-----------------+------
0      | 8    | ts_delta_ns     | u64
8      | 8    | flow_id         | u64
16     | 8    | event_type      | u64
24     | 8    | payload         | u64
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

### ojo (CLI tool)

**Single-threaded processing**:
- One file at a time (no concurrent file processing)
- Database writer in WAL mode for better concurrency
- Can process files while client is writing new ones
- Web server: each HTTP request is independent with database connections from pool

## Failure Modes and Recovery

### Buffer Overflow (Client)
- When buffer is full, increment dropped_count
- Drop the event and move on
- Flusher records EVENTS_DROPPED event

### File System Issues
- **No space**: Flusher logs error, continues
- **Permission denied**: Fatal error on init
- **Staging rename fails**: Fatal error to avoid data loss

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

Potential database backends to support:
- SQLite - Simple, embedded, good for single-user scenarios
- DuckDB - Analytical queries, columnar storage
- Apache Arrow/Parquet - Language-agnostic, excellent for analytics
- InfluxDB - Time-series optimized
- ClickHouse - Distributed analytical queries

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
- Database injection prevention (parameterized queries)
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
ojo watch (terminal)
  ↓
ojo serve (terminal)
  ↓
http://localhost:8080 (browser)
```

### Production
```
ojo-client (embedded)
  ↓
shared/network file system
  ↓
ojo watch (systemd service)
  ↓
ojo serve (systemd service) + nginx
  ↓
https://traces.example.com (browser)
```

### Distributed
```
Multiple clients on different hosts
  ↓
Central NFS/S3 mount
  ↓
ojo watch (processes files)
  ↓
Database
  ↓
ojo serve cluster
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
