//! Simple example of using ojo-client
//!
//! This example demonstrates:
//! - Creating a tracer
//! - Recording packet events
//! - Recording stream events
//! - Recording congestion control events
//!
//! Run with: cargo run --example simple

use ojo_client::{Builder, EventRecord};
use std::{
    thread,
    time::{Duration, Instant},
};

mod events {
    include!(concat!(env!("OUT_DIR"), "/events.rs"));
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Ojo Simple Example");
    println!("==================\n");

    let trace_dir = if let Some(dir) = std::env::args().nth(1) {
        std::path::PathBuf::from(dir)
    } else {
        std::env::temp_dir().join("ojo-example")
    };

    // Create a temporary directory for traces
    std::fs::create_dir_all(&trace_dir)?;

    println!("Trace directory: {:?}\n", trace_dir);

    // Configure the tracer
    let config = Builder::default()
        .output_dir(&trace_dir)
        .buffer_size(10 * 1024 * 1024) // 10 MiB for example
        .flush_interval(Duration::from_millis(500)) // Flush every 500ms
        .schema(&events::SCHEMA);

    // Create the tracer
    println!("Creating tracer...");
    let tracer = config.build()?;
    println!("Tracer initialized!\n");

    let start = Instant::now();
    let flow_id = 1;

    // Helper to get timestamp delta
    let ts = || start.elapsed().as_nanos() as u64;

    // Simulate a simple connection with packet exchanges
    println!("Simulating connection (flow_id = 1)...");

    // Initial congestion window
    tracer.record(EventRecord {
        ts_delta_ns: ts(),
        flow_id,
        event_type: events::CWND_UPDATED,
        payload: 10_000,
    });

    // Send some packets
    for packet_num in 1..=10 {
        println!("  Sending packet {}", packet_num);
        tracer.record(EventRecord {
            ts_delta_ns: ts(),
            flow_id,
            event_type: events::PACKET_SENT,
            payload: packet_num,
        });
        thread::sleep(Duration::from_millis(10));
    }

    // Acknowledge some packets
    for packet_num in 1..=7 {
        println!("  Packet {} acknowledged", packet_num);
        tracer.record(EventRecord {
            ts_delta_ns: ts(),
            flow_id,
            event_type: events::PACKET_ACKED,
            payload: packet_num,
        });
        thread::sleep(Duration::from_millis(5));
    }

    // Packet loss
    println!("  Packet 8 lost (timeout)");
    tracer.record(EventRecord {
        ts_delta_ns: ts(),
        flow_id,
        event_type: events::PACKET_LOST_TIMEOUT,
        payload: 8,
    });

    // Update congestion window after loss
    tracer.record(EventRecord {
        ts_delta_ns: ts(),
        flow_id,
        event_type: events::CWND_UPDATED,
        payload: 5_000,
    });

    // Open a stream
    println!("\nOpening stream (flow_id = 2, stream_id = 10)...");
    let stream_flow_id = 2;
    let stream_id = 10;
    tracer.record(EventRecord {
        ts_delta_ns: ts(),
        flow_id: stream_flow_id,
        event_type: events::STREAM_OPENED,
        payload: stream_id,
    });

    // Send more packets after recovery
    for packet_num in 11..=15 {
        println!("  Sending packet {}", packet_num);
        tracer.record(EventRecord {
            ts_delta_ns: ts(),
            flow_id,
            event_type: events::PACKET_SENT,
            payload: packet_num,
        });
        thread::sleep(Duration::from_millis(10));
    }

    println!("\nExample complete!");
    println!("\nTo view these traces:");
    println!(
        "1. Run: cargo run --bin ojo watch --input-dir {:?} --db-path /tmp/traces.db",
        trace_dir.join("output")
    );
    println!("2. Run: cargo run --bin ojo serve --db-path /tmp/traces.db");
    println!("3. Open: http://localhost:8080\n");

    // Tracer will flush on drop (via Arc strong count check)
    drop(tracer);

    println!("Waiting for final flush...");
    thread::sleep(Duration::from_secs(1));

    Ok(())
}
