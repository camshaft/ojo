use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use core::fmt;
use duckdb::Connection;
use futures::{sink::SinkExt, stream::StreamExt};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt::Write, sync::Arc};
use tokio::sync::{Mutex, Notify, mpsc::UnboundedSender};
use tower_http::services::ServeDir;
use tracing::{error, info};

struct AppState {
    conn: Arc<Mutex<Connection>>,
    db_update_notify: Arc<Notify>,
}

use axum::response::Html;

pub async fn serve(
    conn: Connection,
    db_update_notify: Arc<Notify>,
    host: String,
    port: u16,
) -> anyhow::Result<()> {
    let state = Arc::new(AppState {
        conn: Arc::new(Mutex::new(conn)),
        db_update_notify,
    });

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/ws", get(ws_handler))
        .nest_service(
            "/assets",
            ServeDir::new(concat!(env!("CARGO_MANIFEST_DIR"), "/static/assets")),
        )
        .fallback(get(index_handler))
        .with_state(state);

    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("Server listening on {}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(debug_assertions)]
async fn index_handler() -> Html<String> {
    Html(
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/static/index.html")).unwrap(),
    )
}

#[cfg(not(debug_assertions))]
async fn index_handler() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case", tag = "type")]
enum ClientMessage {
    Subscribe { id: u32, query: Query },
    Unsubscribe { id: u32 },
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case", tag = "kind")]
enum Query {
    Flows {
        #[serde(default)]
        event_ids: Vec<MaybeBigInt>,
    },
    EventTypes,
    Stats {
        #[serde(default)]
        event_ids: Vec<MaybeBigInt>,
    },
    Events {
        #[serde(default)]
        flow_ids: Vec<[MaybeBigInt; 2]>,
        #[serde(default)]
        event_ids: Vec<MaybeBigInt>,
    },
}

impl fmt::Display for Query {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Query::Flows { event_ids } => {
                write!(f, "SELECT batch_id, flow_id, min(ts_delta_ns) as start_ts, max(ts_delta_ns) as end_ts, count(*) as event_count FROM events")?;
                
                if !event_ids.is_empty() {
                    write!(f, " WHERE event_type IN (")?;
                    for (idx, id) in event_ids.iter().enumerate() {
                        let _ = write!(f, "{id}");
                        if idx + 1 != event_ids.len() {
                            f.write_char(',')?;
                        }
                    }
                    write!(f, ")")?;
                }
                
                write!(f, " GROUP BY (batch_id, flow_id) ORDER BY start_ts DESC")?;
                Ok(())
            }
            Query::EventTypes => "SELECT * FROM event_schemas".fmt(f),
            Query::Stats { event_ids } => {
                write!(f, "SELECT event_type, count(*) as count FROM events")?;
                
                if !event_ids.is_empty() {
                    write!(f, " WHERE event_type IN (")?;
                    for (idx, id) in event_ids.iter().enumerate() {
                        let _ = write!(f, "{id}");
                        if idx + 1 != event_ids.len() {
                            f.write_char(',')?;
                        }
                    }
                    write!(f, ")")?;
                }
                
                write!(f, " GROUP BY event_type")?;
                Ok(())
            }
            Query::Events {
                flow_ids,
                event_ids,
            } => {
                write!(f, "SELECT ts_delta_ns, batch_id, flow_id, event_type, primary_value, secondary_value FROM events")?;

                let mut clauses: Vec<String> = Vec::new();
                if !flow_ids.is_empty() {
                    let mut clause = String::from("(batch_id, flow_id) IN (");
                    for (idx, [batch_id, flow_id]) in flow_ids.iter().enumerate() {
                        let _ = write!(clause, "({batch_id}, {flow_id})");
                        if idx + 1 != flow_ids.len() {
                            clause.push(',');
                        }
                    }
                    clause.push(')');
                    clauses.push(clause);
                }
                if !event_ids.is_empty() {
                    let mut clause = String::from("event_type IN (");
                    for (idx, id) in event_ids.iter().enumerate() {
                        let _ = write!(clause, "{id}");
                        if idx + 1 != event_ids.len() {
                            clause.push(',');
                        }
                    }
                    clause.push(')');
                    clauses.push(clause);
                }

                if !clauses.is_empty() {
                    write!(f, " WHERE {}", clauses.join(" AND "))?;
                }

                write!(f, " ORDER BY ts_delta_ns")?;

                Ok(())
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct MaybeBigInt(u64);

impl<'de> Deserialize<'de> for MaybeBigInt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = MaybeBigInt;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string or number representing a u64")
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(MaybeBigInt(value))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                value
                    .parse::<u64>()
                    .map(MaybeBigInt)
                    .map_err(|_| E::custom(format!("failed to parse '{}' as u64", value)))
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

impl Serialize for MaybeBigInt {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl fmt::Display for MaybeBigInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Debug for MaybeBigInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "snake_case", tag = "type")]
enum ServerMessage {
    Update {
        id: u32,
        #[serde(serialize_with = "as_base64")]
        data: Arc<[u8]>,
    },
    Error {
        message: String,
    },
}

fn as_base64<S>(key: &[u8], serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use base64::{Engine as _, engine::general_purpose};
    serializer.serialize_str(&general_purpose::STANDARD.encode(key))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ServerMessage>();

    // Dedicated connection for this websocket session
    let conn = {
        let lock = state.conn.lock().await;
        match lock.try_clone() {
            Ok(c) => Arc::new(std::sync::Mutex::new(c)),
            Err(e) => {
                error!("Failed to clone connection: {}", e);
                return;
            }
        }
    };

    // Channel to send commands to the query runner
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<ClientMessage>();

    // Task to forward mpsc messages to websocket
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let json = match serde_json::to_string(&msg) {
                Ok(j) => j,
                Err(e) => {
                    error!("Failed to serialize message: {}", e);
                    continue;
                }
            };
            if sender.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }
    });

    // Task to handle incoming messages
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Text(text) = msg {
                match serde_json::from_str::<ClientMessage>(&text) {
                    Ok(msg) => {
                        if cmd_tx.send(msg).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Failed to parse message {text:?}: {}", e);
                    }
                }
            } else if let Message::Close(_) = msg {
                break;
            }
        }
    });

    // Task to watch for DB updates and run queries
    let runner_task = tokio::spawn(async move {
        let mut active_queries = HashMap::<u32, (Query, Arc<[u8]>)>::new();

        loop {
            tokio::select! {
                _ = state.db_update_notify.notified() => {
                    for (id, (query, prev)) in active_queries.iter_mut() {
                        match run_query(conn.clone(), *id, query, tx.clone(), Some(prev.clone())).await {
                            Ok(new_data) => {
                                *prev = new_data;
                            }
                            Err(e) => {
                                error!("Failed to run query {:?}: {}", query, e);
                            }
                        }
                    }
                }
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(ClientMessage::Subscribe { id, query }) => {
                            info!("Client subscribed to {:?}", query);
                            match run_query(conn.clone(), id, &query, tx.clone(), None).await {
                                Ok(data) => {
                                    active_queries.insert(id, (query, data));
                                }
                                Err(e) => {
                                    error!("Failed to run query {:?}: {}", query, e);
                                    let _ = tx.send(ServerMessage::Error {
                                        message: e.to_string(),
                                    });
                                }
                            }
                        }
                        Some(ClientMessage::Unsubscribe { id }) => {
                            info!("Client unsubscribed from {:?}", id);
                            active_queries.remove(&id);
                        }
                        None => break,
                    }
                }
            }
        }
    });

    // Wait for any task to clear up
    let _ = tokio::join!(send_task, recv_task, runner_task);
}

async fn run_query(
    conn: Arc<std::sync::Mutex<Connection>>,
    id: u32,
    query: &Query,
    tx: UnboundedSender<ServerMessage>,
    prev: Option<Arc<[u8]>>,
) -> anyhow::Result<Arc<[u8]>> {
    let sql = query.to_string();

    let arrow_batch = tokio::task::spawn_blocking(move || {
        let conn = conn.lock().unwrap();
        let mut stmt = conn.prepare(&sql)?;
        let arrow = stmt.query_arrow([])?;

        let batches: Vec<arrow::array::RecordBatch> = arrow.collect::<Vec<_>>();

        // Serialize batches to IPC
        let mut buf = Vec::new();
        {
            let schema = if batches.is_empty() {
                // Return empty buffer if no data
                return Ok(Vec::new());
            } else {
                batches[0].schema()
            };

            let mut writer = arrow::ipc::writer::FileWriter::try_new(&mut buf, &schema)?;
            for batch in batches {
                writer.write(&batch)?;
            }
            writer.finish()?;
        }
        Ok::<Vec<u8>, anyhow::Error>(buf)
    })
    .await??;

    let arrow_batch: Arc<[u8]> = Arc::from(arrow_batch);

    if let Some(prev_data) = prev
        && *prev_data == *arrow_batch
    {
        // No change
        return Ok(prev_data);
    }

    tx.send(ServerMessage::Update {
        id,
        data: arrow_batch.clone(),
    })?;

    Ok(arrow_batch)
}
