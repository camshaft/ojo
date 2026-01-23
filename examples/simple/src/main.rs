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
    ops::Range,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

mod events {
    include!(concat!(env!("OUT_DIR"), "/events.rs"));
}

static FLOW_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

fn random_delay() {
    let delay = rand::random_range(0..10);
    if delay > 0 {
        // Introduce a small random delay to simulate network latency
        thread::sleep(Duration::from_millis(delay));
    }
}

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

    // Send some packets
    let packet_count = rand::random_range(2..2000);

    let (tx, rx) = std::sync::mpsc::channel::<Range<u64>>();
    let (lost_tx, lost_rx) = std::sync::mpsc::channel::<Range<u64>>();

    std::thread::scope(|s| {
        // Main transmission thread
        s.spawn({
            let tx = tx.clone();
            move || {
                let mut offset = 0;
                for _ in 1..=packet_count {
                    let packet_size = rand::random_range(500..1500);
                    let start = offset;
                    let end = start + packet_size;
                    offset = end;

                    tracer.record(EventRecord {
                        ts_delta_ns: ts(),
                        flow_id,
                        event_type: events::OFFSET_SENT,
                        primary: start,
                        secondary: end,
                    });
                    tx.send(start..end).unwrap();

                    random_delay();
                }
            }
        });

        // Retransmission thread
        s.spawn(move || {
            while let Ok(range) = lost_rx.recv() {
                // Retransmit after some delay
                random_delay();
                tracer.record(EventRecord {
                    ts_delta_ns: ts(),
                    flow_id,
                    event_type: events::OFFSET_RETRANSMITTED,
                    primary: range.start,
                    secondary: range.end,
                });
                tx.send(range).unwrap();
            }
        });

        // Main ACK processing thread
        s.spawn(move || {
            while let Ok(range) = rx.recv() {
                // Randomly decide if the packet is lost
                if rand::random::<f32>() < 0.05 {
                    lost_tx.send(range).unwrap();
                    continue;
                }

                let start = range.start;
                let end = range.end;

                tracer.record(EventRecord {
                    ts_delta_ns: ts(),
                    flow_id,
                    event_type: events::OFFSET_ACKED,
                    primary: start,
                    secondary: end,
                });
                random_delay();
            }
        });
    });
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
