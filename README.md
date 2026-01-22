# Ojo

**Transport Protocol Event Tracing System**

Ojo is a low-overhead, high-performance event tracing system designed for debugging and analyzing transport protocol behavior. It provides a complete solution for collecting, transforming, and visualizing network protocol events.

## Features

- **Lock-free Event Collection**: Zero-allocation hot path
- **Queryable Storage**: Transform binary traces into queryable database
- **Browser-Based Explorer**: Visualize packet flows, streams, and congestion control
- **Real-time Monitoring**: Watch directories for new traces and update live
- **Fixed-size Binary Format**: Predictable 24-byte records for zero-copy parsing
- **Thread-safe**: Multi-writer, single-reader architecture

## Architecture

Ojo consists of two main components:

### 1. **ojo-client** - Client Library

Rust library for collecting and writing events to binary files with minimal overhead.

### 2. **ojo** - CLI Tool

Combined tool with subcommands:

- `ojo watch` - Monitors trace directories and transforms binary data into queryable database
- `ojo serve` - Web-based interface for querying and visualizing trace data in real-time

## Quick Start

### Installation

```bash
# Add to your Cargo.toml
[dependencies]
ojo-client = "0.1"
```

### Basic Usage

```rust
use ojo_client::{Tracer, TracerConfig, Event, event_type};

// Create a tracer
let config = TracerConfig::default()
    .with_output_dir("./traces");
let tracer = Tracer::new(config)?;

// Record events
tracer.record(Event {
    timestamp_ns: 12345,
    flow_id: 1,
    event_type: event_type::PACKET_SENT,
    payload: 100,
});
```

### Running the Watcher

```bash
# Watch a directory and transform traces
ojo watch --input-dir ./traces/output \
          --db-path ./traces.db
```

### Running the Explorer

```bash
# Start the web interface
ojo serve --db-path ./traces.db --port 8080

# Open http://localhost:8080 in your browser
```

## Event Types

Ojo supports several types of event tracking:

- **Packet Events**: Create, send, ACK, loss, retransmit
- **Stream Events**: Open, FIN sent, FIN ACK
- **Flow Control**: Connection and stream-level max data updates
- **Congestion Control**: CWND, ssthresh, loss detection state

## Documentation

- API Documentation - Run `cargo doc --open`

## Use Cases

- **Protocol Debugging**: Trace packet lifecycle and identify issues
- **Performance Analysis**: Analyze congestion control and flow control behavior
- **Visualization**: Generate packet progression graphs and timelines
- **Research**: Study transport protocol behavior under various conditions

## Contributing

Contributions are welcome! Please check out our [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
