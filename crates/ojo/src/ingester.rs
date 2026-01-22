use crate::loader;
use anyhow::{Context, Result};
use duckdb::Connection;
use parquet::arrow::ArrowWriter;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;
use tracing::{debug, error, info};

pub struct Ingester {
    conn: Connection,
    raw_dir: PathBuf,
    data_dir: PathBuf,
    schema_dir: PathBuf,
    staging_dir: PathBuf,
}

impl Ingester {
    pub fn new(trace_dir: PathBuf, conn: Connection) -> Result<Self> {
        let raw_dir = trace_dir.join("raw");
        let data_dir = trace_dir.join("data");
        let schema_dir = trace_dir.join("schema");
        let staging_dir = trace_dir.join("staging");
        std::fs::create_dir_all(&raw_dir)?;
        std::fs::create_dir_all(&data_dir)?;
        std::fs::create_dir_all(&schema_dir)?;
        std::fs::create_dir_all(&staging_dir)?;

        let ingester = Self {
            conn,
            raw_dir,
            data_dir,
            schema_dir,
            staging_dir,
        };
        ingester.init_db()?;
        Ok(ingester)
    }

    fn init_db(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            -- Tracks ingested trace files but not yet rolled up
            CREATE TABLE IF NOT EXISTS pending_rollup (
                filename TEXT PRIMARY KEY,
            );
            -- Tracks event schemas
            CREATE TABLE IF NOT EXISTS event_schemas (
                id UBIGINT,
                name TEXT,
                category TEXT,
                description TEXT,
                module TEXT,
                PRIMARY KEY (id)
            );
            -- Event data
            CREATE TABLE IF NOT EXISTS events (
                ts_delta_ns UBIGINT,
                batch_id UBIGINT,
                flow_id UBIGINT,
                event_type UBIGINT,
                payload UBIGINT,
                FOREIGN KEY (event_type) REFERENCES event_schemas(id)
            );
            ",
        )?;

        {
            let snapshot_path = self.data_dir.join("event_schemas.parquet");
            if snapshot_path.exists() {
                info!("Loading snapshot.parquet into events table...");
                self.conn.execute(
                    "INSERT OR REPLACE INTO events SELECT * FROM read_parquet(?)",
                    [snapshot_path.to_string_lossy()],
                )?;
            }
        }

        Ok(())
    }

    pub fn ingest_all(&mut self) -> Result<()> {
        self.ingest_schemas()?;
        self.ingest_traces()?;
        Ok(())
    }

    pub fn ingest_schemas(&self) -> Result<()> {
        if !self.schema_dir.exists() {
            return Ok(());
        }

        for entry in std::fs::read_dir(&self.schema_dir)? {
            let entry = entry?;
            let path = entry.path();
            if let Err(e) = self.ingest_schema_file(&path) {
                error!("Failed to ingest schema file {:?}: {}", path, e);
            }
        }
        Ok(())
    }

    fn ingest_schema_file(&self, path: &Path) -> Result<()> {
        let filename = path.file_name().unwrap_or_default().to_string_lossy();

        if !(filename.starts_with("event_schema_") && filename.ends_with(".json")) {
            return Ok(());
        }

        debug!("Loading schema {:?}", path);
        let batch = loader::load_schema_file(path)?;
        // Write to temp parquet to load into DuckDB
        let mut file = NamedTempFile::with_suffix_in(".parquet", &self.staging_dir)?;
        let mut writer = ArrowWriter::try_new(&mut file, batch.schema(), None)?;
        writer.write(&batch)?;
        writer.close()?;

        // Load from parquet
        self.conn.execute(
            "INSERT OR REPLACE INTO event_schemas SELECT * FROM read_parquet(?)",
            [file.path().to_string_lossy()],
        )?;

        Ok(())
    }

    pub fn ingest_traces(&mut self) -> Result<()> {
        if !self.raw_dir.exists() {
            return Ok(());
        }

        for entry in std::fs::read_dir(&self.raw_dir)? {
            let entry = entry?;
            let path = entry.path();
            match self.ingest_raw_trace_file(&path) {
                Ok(_ingested) => {}
                Err(e) => {
                    error!("Failed to ingest trace file {:?}: {}", path, e);
                }
            }
        }

        Ok(())
    }

    fn ingest_raw_trace_file(&mut self, path: &Path) -> Result<bool> {
        if path.extension().is_none_or(|ext| ext != "bin") {
            return Ok(false);
        }
        let filename = path.file_name().unwrap().to_string_lossy().to_string();

        // Check if already processed
        let exists: bool = self.conn.query_row(
            "SELECT count(*) > 0 FROM pending_rollup WHERE filename = ?",
            [&filename],
            |row| row.get(0),
        )?;

        if exists {
            return Ok(false);
        }

        // If parquet doesn't exist, create it
        debug!("Ingesting {:?}", path);
        let batch = loader::load_trace_file(path)?;

        // create a transaction to insert into pending_rollup and load the events
        let tx = self.conn.transaction()?;

        tx.appender("events")?.append_record_batch(batch)?;

        tx.execute(
            "INSERT INTO pending_rollup (filename) VALUES (?)",
            [&filename],
        )?;

        tx.commit()?;

        Ok(true)
    }

    pub fn export_snapshot(&mut self) -> Result<()> {
        // Export the events table to a consolidated parquet file
        let file = NamedTempFile::with_suffix_in("aggregated.parquet", &self.staging_dir)?;

        let tx = self.conn.transaction()?;
        tx.execute(
            "COPY (SELECT * FROM events) TO ? (FORMAT PARQUET, COMPRESSION 'ZSTD')",
            [file.path().to_string_lossy()],
        )?;

        // Iterate over all of the files that we've ingested and delete them
        let mut stmt = tx.prepare("SELECT filename FROM pending_rollup")?;
        let filenames: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<_>, duckdb::Error>>()
            .context("Querying ingested files for snapshot cleanup")?;

        for filename in filenames {
            let path = self.raw_dir.join(filename);
            if !path.exists() {
                continue;
            }
            std::fs::remove_file(path)?;
        }

        // Delete all entries from pending_rollup
        tx.execute("DELETE FROM pending_rollup", [])?;

        let new_path = self.data_dir.join("snapshot.parquet");
        file.persist(new_path)?;

        tx.commit()?;

        Ok(())
    }
}
