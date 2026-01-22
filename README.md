# Ojo 👁️

**Transport Protocol Event Tracing System**

Ojo is a low-overhead, high-performance event tracing system designed for debugging and analyzing transport protocol behavior. It provides a complete solution for collecting, transforming, and visualizing network protocol events.

## Features

- **🚀 Lock-free Event Collection**: Zero-allocation hot path with < 100ns per event
- **📊 Queryable Storage**: Transform binary traces into SQLite for powerful queries
- **🌐 Browser-Based Explorer**: Visualize packet flows, streams, and congestion control
- **🔄 Real-time Monitoring**: Watch directories for new traces and update live
- **📦 Fixed-size Binary Format**: Predictable 24-byte records for zero-copy parsing
- **🧵 Thread-safe**: Multi-writer, single-reader architecture

## Architecture

Ojo consists of three main components:

```
┌─────────────────┐     Binary Files      ┌──────────────────┐
│   ojo-client    │ ──────────────────▶   │   ojo-watcher    │
│  (Rust Library) │   (staging/ → output/) │ (Background Svc) │
└─────────────────┘                        └────────┬─────────┘
                                                    │
                                            SQLite Database
                                                    │
                                          ┌─────────▼─────────┐
                                          │   ojo-explorer    │
                                          │  (Web Interface)  │
                                          └───────────────────┘
```

### 1. **ojo-client** - Client Library
Rust library for collecting and writing events to binary files with minimal overhead.

### 2. **ojo-watcher** - Watcher Process
Background service that monitors trace directories and transforms binary data into a queryable SQLite database.

### 3. **ojo-explorer** - Browser Explorer
Web-based interface for querying and visualizing trace data in real-time.

## Quick Start

### Installation

```bash
# Add to your Cargo.toml
[dependencies]
ojo-client = "0.1"
```

### Basic Usage

```rust
use ojo_client::{Tracer, TracerConfig};

// Create a tracer
let config = TracerConfig::default()
    .with_output_dir("./traces");
let tracer = Tracer::new(config)?;

// Record events
tracer.record_packet_sent(flow_id, packet_number);
tracer.record_packet_acked(flow_id, packet_number);
tracer.record_stream_opened(flow_id, stream_id);
```

### Running the Watcher

```bash
# Watch a directory and transform traces
ojo-watcher --input-dir ./traces/output \
            --db-path ./traces.db \
            --watch
```

### Running the Explorer

```bash
# Start the web interface
ojo-explorer --db-path ./traces.db --port 8080

# Open http://localhost:8080 in your browser
```

## Event Types

Ojo supports comprehensive event tracking:

- **Packet Events**: Create, send, ACK, loss, retransmit
- **Stream Events**: Open, FIN sent, FIN ACK, parent linking
- **Flow Control**: Connection and stream-level max data updates
- **Congestion Control**: CWND, ssthresh, loss detection state

## Binary Format

Ojo uses a efficient binary format with:
- 24-byte header (magic, version, timestamp)
- 24-byte fixed-size event records
- Little-endian encoding
- Streaming-friendly design

See [PROJECT_PLAN.md](PROJECT_PLAN.md) for detailed format specification.

## Performance

Designed for minimal overhead:
- **< 100 ns** per event on hot path
- **Zero allocations** during event recording
- **1M+ events/second** with concurrent writers
- **Lock-free** ring buffer operations

## Documentation

- [Project Plan](PROJECT_PLAN.md) - Comprehensive design and implementation plan
- [Design Specification](https://gist.githubusercontent.com/camshaft/fb54b7f99677e170d5ee1588744ce339/raw/README.md) - Binary format specification
- API Documentation - Run `cargo doc --open`

## Project Status

🚧 **Under Development** - See [PROJECT_PLAN.md](PROJECT_PLAN.md) for roadmap

Current phase: Foundation & Setup

## Use Cases

- **Protocol Debugging**: Trace packet lifecycle and identify issues
- **Performance Analysis**: Analyze congestion control and flow control behavior
- **Visualization**: Generate packet progression graphs and timelines
- **Research**: Study transport protocol behavior under various conditions

## Contributing

Contributions are welcome! Please check out our [PROJECT_PLAN.md](PROJECT_PLAN.md) for the roadmap and architecture details.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

Built following the Transport Protocol Event Tracing Format specification for low-overhead, high-fidelity protocol event collection.