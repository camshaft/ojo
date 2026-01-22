//! # ojo
//!
//! CLI for the ojo tracer

use clap::{Parser, Subcommand};
use duckdb::Connection;
use std::path::PathBuf;
use tracing::{error, info};

mod ingester;
mod loader;

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

            ingester_thread(ingester);
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

            // Run ingestion in background
            let ingester =
                Ingester::new(ingest_input, ingester_conn).expect("Failed to create ingester");
            std::thread::spawn(move || {
                ingester_thread(ingester);
            });

            // TODO: Implement explorer API and web server
            info!("Server starting at http://{}:{}", host, port);

            tokio::signal::ctrl_c().await?;
        }
    }

    Ok(())
}

fn ingester_thread(mut ingester: Ingester) {
    let mut last_snapshot: Option<std::time::Instant> = None;

    loop {
        if let Err(e) = ingester.ingest_all() {
            error!("Ingestion error: {}", e);
        }
        if last_snapshot.is_none_or(|v| v.elapsed().as_secs() >= 10) {
            if let Err(e) = ingester.export_snapshot() {
                error!("Snapshot error: {}", e);
            } else {
                last_snapshot = Some(std::time::Instant::now());
            }
        }
        // TODO use `notify` crate to watch for file changes instead of polling
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
