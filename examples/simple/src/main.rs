#![allow(clippy::manual_is_multiple_of)]
//! Simple example of using ojo-client
//!
//! This example demonstrates:
//! - Creating a tracer
//! - Recording packet events
//! - Recording stream events
//! - Recording congestion control events
//!
//! Run with: cargo run -p simple-example

use ojo_client::{Builder, EventRecord, Tracer};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

mod events {
    include!(concat!(env!("OUT_DIR"), "/events.rs"));
}

static FLOW_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

fn simulate_connection(tracer: &Tracer, start_time: Instant, worker_id: u64) {
    let flow_id = FLOW_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let ts = || start_time.elapsed().as_nanos() as u64;

    // Simulate a simple connection with packet exchanges
    if flow_id % 10 == 0 {
        println!(
            "[Worker {}] Simulating connection (flow_id = {})...",
            worker_id, flow_id
        );
    }

    // Initial congestion window
    tracer.record(EventRecord {
        ts_delta_ns: ts(),
        flow_id,
        event_type: events::CWND_UPDATED,
        payload: 10_000,
    });

    // Send some packets
    let packet_count = 5 + (flow_id % 10); // variable packet count
    for packet_num in 1..=packet_count {
        tracer.record(EventRecord {
            ts_delta_ns: ts(),
            flow_id,
            event_type: events::PACKET_SENT,
            payload: packet_num,
        });
        thread::sleep(Duration::from_millis(5));
    }

    // Acknowledge some packets
    for packet_num in 1..=(packet_count - 2) {
        tracer.record(EventRecord {
            ts_delta_ns: ts(),
            flow_id,
            event_type: events::PACKET_ACKED,
            payload: packet_num,
        });
        thread::sleep(Duration::from_millis(2));
    }

    // Occasional packet loss
    if flow_id % 3 == 0 {
        let lost_packet = packet_count - 1;
        tracer.record(EventRecord {
            ts_delta_ns: ts(),
            flow_id,
            event_type: events::PACKET_LOST_TIMEOUT,
            payload: lost_packet,
        });

        // Update congestion window after loss
        tracer.record(EventRecord {
            ts_delta_ns: ts(),
            flow_id,
            event_type: events::CWND_UPDATED,
            payload: 5_000,
        });
    }

    // Open a stream occasionally
    if flow_id % 5 == 0 {
        let stream_id = flow_id * 10;
        tracer.record(EventRecord {
            ts_delta_ns: ts(),
            flow_id,
            event_type: events::STREAM_OPENED,
            payload: stream_id,
        });
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Ojo Continuous Simulation");
    println!("=========================\n");

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
    let tracer = Arc::new(config.build()?);
    println!("Tracer initialized! Starting simulation workers...\n");
    println!("Press Ctrl+C to stop.");
    println!("To view traces while running:");
    println!(
        "  cargo run --bin ojo watch --input-dir {:?}",
        trace_dir.join("output")
    );

    let start_time = Instant::now();
    let mut handles = vec![];

    // Spawn workers to generate load
    let worker_count = 4;
    for i in 0..worker_count {
        let tracer = tracer.clone();
        handles.push(thread::spawn(move || {
            loop {
                simulate_connection(&tracer, start_time, i);
                // Random-ish sleep
                let sleep_ms = 100 + (start_time.elapsed().as_millis() as u64 % 500);
                thread::sleep(Duration::from_millis(sleep_ms));
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    Ok(())
}
