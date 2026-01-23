//! # ojo
//!
//! CLI for the ojo tracer

use clap::{Parser, Subcommand};
use duckdb::Connection;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use std::{path::PathBuf, sync::Arc, sync::mpsc, time::Duration};
use tokio::sync::Notify;
use tracing::{debug, error, info};

mod ingester;
mod loader;
mod server;

use ingester::Ingester;

#[derive(Parser, Debug)]
#[command(name = "ojo")]
#[command(about = "Transport protocol event tracer", long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Watch trace files and transform them into queryable format (headless mode)
    Watch {
        /// Directory containing trace files to process
        #[arg(long, default_value = "./traces")]
        trace_dir: PathBuf,
    },

    /// Serve the web interface for exploring traces
    Serve {
        /// Directory containing trace files to process
        #[arg(long, default_value = "./traces")]
        trace_dir: PathBuf,

        /// Port to bind the web server to
        #[arg(long, short, default_value = "8080")]
        port: u16,

        /// Host to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    match args.command {
        Commands::Watch { trace_dir } => {
            info!("Starting ojo watch mode");
            info!("  Input directory: {:?}", trace_dir);

            std::fs::create_dir_all(&trace_dir)?;
            let trace_dir = trace_dir.canonicalize()?;

            let conn = Connection::open_in_memory()?;
            let ingester = Ingester::new(trace_dir, conn)?;

            let notify = Arc::new(Notify::new());
            ingester_thread(ingester, notify);
        }

        Commands::Serve {
            trace_dir,
            port,
            host,
        } => {
            info!("Starting ojo serve mode");
            info!("  Trace directory: {:?}", trace_dir);
            info!("  Binding to: {}:{}", host, port);

            std::fs::create_dir_all(&trace_dir)?;
            let trace_dir = trace_dir.canonicalize()?;

            let db_path = trace_dir.join("ojo.db");
            info!("  Database path: {:?}", db_path);
            let db_file = tempfile::TempPath::from_path(db_path);

            let ingest_input = trace_dir.clone();

            let conn = Connection::open(&db_file).expect("Failed to open database connection");

            let ingester_conn = conn
                .try_clone()
                .expect("Failed to clone database connection");

            let notify = Arc::new(Notify::new());
            let notify_clone = notify.clone();

            // Run ingestion in background
            let ingester =
                Ingester::new(ingest_input, ingester_conn).expect("Failed to create ingester");
            std::thread::spawn(move || {
                ingester_thread(ingester, notify_clone);
            });

            server::serve(conn, notify, host, port).await?;
        }
    }

    Ok(())
}

fn ingester_thread(mut ingester: Ingester, notify: Arc<Notify>) {
    let mut last_snapshot: Option<std::time::Instant> = None;

    // Create a channel for receiving file system events
    let (tx, rx) = mpsc::channel();

    // Create a watcher object
    let mut watcher = match RecommendedWatcher::new(tx, Config::default()) {
        Ok(w) => w,
        Err(e) => {
            error!("Failed to create file watcher: {}", e);
            return;
        }
    };

    // Watch the raw and schema directories
    let raw_dir = ingester.raw_dir();
    let schema_dir = ingester.schema_dir();

    if let Err(e) = watcher.watch(raw_dir, RecursiveMode::NonRecursive) {
        error!("Failed to watch raw directory: {}", e);
        return;
    }
    info!("Watching directory for changes: {:?}", raw_dir);

    if let Err(e) = watcher.watch(schema_dir, RecursiveMode::NonRecursive) {
        error!("Failed to watch schema directory: {}", e);
        return;
    }
    info!("Watching directory for changes: {:?}", schema_dir);

    // Do an initial ingestion
    match ingester.ingest_all() {
        Ok(true) => {
            debug!("Initial data ingested, notifying listeners");
            notify.notify_waiters();
        }
        Ok(false) => {}
        Err(e) => {
            error!("Initial ingestion error: {}", e);
        }
    }

    loop {
        // Wait for file system events with a timeout to allow periodic snapshot exports
        let event_result = rx.recv_timeout(Duration::from_secs(5));

        match event_result {
            Ok(Ok(event)) => {
                debug!("File system event: {:?}", event);
                // Process the ingestion
                match ingester.ingest_all() {
                    Ok(true) => {
                        debug!("New data ingested, notifying listeners");
                        notify.notify_waiters();
                    }
                    Ok(false) => {}
                    Err(e) => {
                        error!("Ingestion error: {}", e);
                    }
                }
            }
            Ok(Err(e)) => {
                error!("Watch error: {:?}", e);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Timeout is expected, just continue
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                error!("Watcher disconnected");
                break;
            }
        }

        // Periodically export snapshots
        if last_snapshot.is_none_or(|v| v.elapsed().as_secs() >= 10) {
            if let Err(e) = ingester.export_snapshot() {
                error!("Snapshot error: {}", e);
            } else {
                last_snapshot = Some(std::time::Instant::now());
            }
        }
    }
}
