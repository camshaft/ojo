use anyhow::{Result, anyhow, bail};
use arrow::{
    array::{RecordBatch, StringArray, UInt64Array},
    datatypes::{DataType, Field, Schema},
};
use ojo_client::{EventRecord, FileHeader};
use std::{fs::File, io::Read, path::Path, sync::Arc};
use zerocopy::FromBytes;

pub fn load_trace_file(path: &Path) -> Result<RecordBatch> {
    let mut file = File::open(path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    if buffer.len() < std::mem::size_of::<FileHeader>() {
        bail!("File too short to contain header");
    }

    let (header_bytes, content_bytes) = buffer.split_at(std::mem::size_of::<FileHeader>());
    let header =
        FileHeader::read_from(header_bytes).ok_or_else(|| anyhow!("Failed to read header"))?;

    if &header.magic != b"ojo\0" {
        bail!("Invalid magic bytes: {:?}", header.magic);
    }

    let record_size = std::mem::size_of::<EventRecord>();
    if content_bytes.len() % record_size != 0 {
        bail!(
            "Content length {} is not a multiple of record size {}",
            content_bytes.len(),
            record_size
        );
    }

    let count = content_bytes.len() / record_size;
    let mut timestamps = Vec::with_capacity(count);
    let mut batch_ids = Vec::with_capacity(count);
    let mut flow_ids = Vec::with_capacity(count);
    let mut event_types = Vec::with_capacity(count);
    let mut payloads = Vec::with_capacity(count);

    // We accept that the buffer might not be aligned, so we copy.
    // In a zero-copy optimization we would mmap and strict align, but file read is simpler.
    for chunk in content_bytes.chunks_exact(record_size) {
        // EventRecord is simple Copy POD
        let record =
            EventRecord::read_from(chunk).ok_or_else(|| anyhow!("Failed to read record"))?;

        // Calculate absolute timestamp in nanoseconds
        let ts = header.batch_start_ns.saturating_add(record.ts_delta_ns);

        timestamps.push(ts);
        batch_ids.push(header.batch_start_ns);
        flow_ids.push(record.flow_id);
        event_types.push(record.event_type);
        payloads.push(record.payload);
    }

    let schema = Schema::new(vec![
        Field::new("timestamp_ns", DataType::UInt64, false),
        Field::new("batch_id", DataType::UInt64, false),
        Field::new("flow_id", DataType::UInt64, false),
        Field::new("event_type", DataType::UInt64, false),
        Field::new("payload", DataType::UInt64, false),
    ]);

    let batch = RecordBatch::try_new(
        Arc::new(schema),
        vec![
            Arc::new(UInt64Array::from(timestamps)),
            Arc::new(UInt64Array::from(batch_ids)),
            Arc::new(UInt64Array::from(flow_ids)),
            Arc::new(UInt64Array::from(event_types)),
            Arc::new(UInt64Array::from(payloads)),
        ],
    )?;

    Ok(batch)
}

#[derive(serde::Deserialize)]
struct JsonSchema {
    events: Vec<JsonEvent>,
}

#[derive(serde::Deserialize)]
struct JsonEvent {
    value: u64,
    name: String,
    category: String,
    description: String,
    module: String,
}

pub fn load_schema_file(path: &Path) -> Result<RecordBatch> {
    let file = File::open(path)?;
    let schema_json: JsonSchema = serde_json::from_reader(file)?;

    let count = schema_json.events.len();
    let mut ids = Vec::with_capacity(count);
    let mut names = Vec::with_capacity(count);
    let mut categories = Vec::with_capacity(count);
    let mut descriptions = Vec::with_capacity(count);
    let mut modules = Vec::with_capacity(count);

    for event in schema_json.events {
        ids.push(event.value);
        names.push(event.name);
        categories.push(event.category);
        descriptions.push(event.description);
        modules.push(event.module);
    }

    let schema = Schema::new(vec![
        Field::new("id", DataType::UInt64, false),
        Field::new("name", DataType::Utf8, false),
        Field::new("category", DataType::Utf8, false),
        Field::new("description", DataType::Utf8, false),
        Field::new("module", DataType::Utf8, false),
    ]);

    let batch = RecordBatch::try_new(
        Arc::new(schema),
        vec![
            Arc::new(UInt64Array::from(ids)),
            Arc::new(StringArray::from(names)),
            Arc::new(StringArray::from(categories)),
            Arc::new(StringArray::from(descriptions)),
            Arc::new(StringArray::from(modules)),
        ],
    )?;

    Ok(batch)
}
