use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use duckdb::Connection;
use futures::{sink::SinkExt, stream::StreamExt};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{Mutex, Notify, mpsc::UnboundedSender};
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
        .with_state(state);

    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("Server listening on {}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}

async fn index_handler() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case", tag = "type", content = "query")]
enum ClientMessage {
    Subscribe(Query),
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case", tag = "kind")]
enum Query {
    Flows,
    EventTypes,
    Stats,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "snake_case", tag = "type")]
enum ServerMessage {
    Update {
        query: Query,
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
                        error!("Failed to parse message: {}", e);
                    }
                }
            } else if let Message::Close(_) = msg {
                break;
            }
        }
    });

    // Task to watch for DB updates and run queries
    let runner_task = tokio::spawn(async move {
        let mut active_queries = HashMap::<Query, Arc<[u8]>>::new();

        loop {
            tokio::select! {
                _ = state.db_update_notify.notified() => {
                    for (query, prev) in active_queries.iter_mut() {
                        match run_query(conn.clone(), query.clone(), tx.clone(), Some(prev.clone())).await {
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
                        Some(ClientMessage::Subscribe(query)) => {
                            info!("Client subscribed to {:?}", query);
                            match run_query(conn.clone(), query.clone(), tx.clone(), None).await {
                                Ok(data) => {
                                    active_queries.insert(query, data);
                                }
                                Err(e) => {
                                    error!("Failed to run query {:?}: {}", query, e);
                                    let _ = tx.send(ServerMessage::Error {
                                        message: e.to_string(),
                                    });
                                }
                            }
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
    query: Query,
    tx: UnboundedSender<ServerMessage>,
    prev: Option<Arc<[u8]>>,
) -> anyhow::Result<Arc<[u8]>> {
    let sql = match query {
        Query::Flows => {
            "SELECT batch_id, flow_id, min(ts_delta_ns) as start_ts, max(ts_delta_ns) as end_ts, count(*) as event_count FROM events GROUP BY (batch_id, flow_id) ORDER BY start_ts DESC"
        }
        Query::EventTypes => "SELECT * FROM event_schemas",
        Query::Stats => "SELECT event_type, count(*) as count FROM events GROUP BY event_type",
    };

    let arrow_batch = tokio::task::spawn_blocking(move || {
        let conn = conn.lock().unwrap();
        let mut stmt = conn.prepare(sql)?;
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
        query: query.clone(),
        data: arrow_batch.clone(),
    })?;

    Ok(arrow_batch)
}
