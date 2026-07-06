use axum::{Json, Router, response::Html, routing::get};
use axum::response::sse::{Event, Sse};
use serde_json::{Value, json};
use std::convert::Infallible;
use tokio_stream::wrappers::ReceiverStream;

async fn get_overview() -> Json<Value> {
    let mut files_indexed = 0_i64;
    let mut symbols_indexed = 0_i64;
    let mut relationships_indexed = 0_i64;
    let mut total_tokens_avoided = 0_i64;
    
    if let Ok(conn) = rusqlite::Connection::open(".codebroker/codebroker.db") {
        files_indexed = conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap_or(0);
        symbols_indexed = conn.query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0)).unwrap_or(0);
        relationships_indexed = conn.query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0)).unwrap_or(0);
        total_tokens_avoided = conn.query_row("SELECT SUM(token_reduction) FROM mcp_analytics_events", [], |r| r.get(0)).unwrap_or(0);
    }
    
    Json(json!({
        "files_indexed": files_indexed,
        "symbols_indexed": symbols_indexed,
        "relationships_indexed": relationships_indexed,
        "total_tokens_avoided": total_tokens_avoided,
    }))
}

async fn get_health() -> Json<Value> {
    let mut cache_hits = 0_i64;
    let mut cache_misses = 0_i64;
    let mut db_size_bytes = 0_u64;

    if let Ok(metadata) = std::fs::metadata(".codebroker/codebroker.db") {
        db_size_bytes = metadata.len();
    }

    if let Ok(conn) = rusqlite::Connection::open(".codebroker/codebroker.db") {
        cache_hits = conn.query_row("SELECT COUNT(*) FROM mcp_analytics_events WHERE cache_hit = 1", [], |r| r.get(0)).unwrap_or(0);
        cache_misses = conn.query_row("SELECT COUNT(*) FROM mcp_analytics_events WHERE cache_hit = 0", [], |r| r.get(0)).unwrap_or(0);
    }

    let hit_rate = if cache_hits + cache_misses > 0 {
        format!("{:.1}%", (cache_hits as f64 / (cache_hits + cache_misses) as f64) * 100.0)
    } else {
        "0%".to_string()
    };

    Json(json!({
        "cache_hit_rate": hit_rate,
        "db_size_bytes": db_size_bytes,
        "status": "healthy"
    }))
}

async fn get_mcp_activity() -> Json<Value> {
    let mut activities = Vec::new();
    if let Ok(conn) = rusqlite::Connection::open(".codebroker/codebroker.db") {
        if let Ok(mut stmt) = conn.prepare("SELECT tool_name, prompt, success, execution_time_ms, delivered_token_count, cache_hit, created_at FROM mcp_analytics_events ORDER BY id DESC LIMIT 50") {
            if let Ok(mut rows) = stmt.query([]) {
                while let Some(row) = rows.next().unwrap_or(None) {
                    activities.push(json!({
                        "tool": row.get::<_, String>(0).unwrap_or_default(),
                        "prompt": row.get::<_, Option<String>>(1).unwrap_or_default().unwrap_or_default(),
                        "success": row.get::<_, bool>(2).unwrap_or(true),
                        "latency_ms": row.get::<_, i64>(3).unwrap_or(0),
                        "tokens": row.get::<_, i64>(4).unwrap_or(0),
                        "cache_hit": row.get::<_, bool>(5).unwrap_or(false),
                        "timestamp": row.get::<_, String>(6).unwrap_or_default()
                    }));
                }
            }
        }
    }
    Json(json!(activities))
}

async fn get_mcp_tools() -> Json<Value> {
    let mut tools = Vec::new();
    if let Ok(conn) = rusqlite::Connection::open(".codebroker/codebroker.db") {
        if let Ok(mut stmt) = conn.prepare("SELECT tool_name, COUNT(*), SUM(execution_time_ms), SUM(token_reduction) FROM mcp_analytics_events GROUP BY tool_name ORDER BY COUNT(*) DESC") {
            if let Ok(mut rows) = stmt.query([]) {
                while let Some(row) = rows.next().unwrap_or(None) {
                    let count: i64 = row.get(1).unwrap_or(0);
                    let sum_time: i64 = row.get(2).unwrap_or(0);
                    let savings: i64 = row.get(3).unwrap_or(0);
                    tools.push(json!({
                        "name": row.get::<_, String>(0).unwrap_or_default(),
                        "calls": count,
                        "avg_latency": if count > 0 { sum_time / count } else { 0 },
                        "tokens_saved": savings
                    }));
                }
            }
        }
    }
    Json(json!(tools))
}

async fn get_graph_snapshot() -> Json<Value> {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    if let Ok(conn) = rusqlite::Connection::open(".codebroker/codebroker.db") {
        // Fetch top 100 symbols by edge count to avoid overwhelming React Flow
        if let Ok(mut stmt) = conn.prepare("
            SELECT s.id, s.name, s.kind, COUNT(e.id) as edge_count
            FROM symbols s
            LEFT JOIN edges e ON s.id = e.source_symbol_id OR s.id = e.target_symbol_id
            GROUP BY s.id
            ORDER BY edge_count DESC
            LIMIT 100
        ") {
            if let Ok(mut rows) = stmt.query([]) {
                while let Some(row) = rows.next().unwrap_or(None) {
                    let id: i64 = row.get(0).unwrap_or(0);
                    nodes.push(json!({
                        "id": id.to_string(),
                        "label": row.get::<_, String>(1).unwrap_or_default(),
                        "type": row.get::<_, String>(2).unwrap_or_default(),
                    }));
                }
            }
        }

        // Fetch edges between these nodes
        let node_ids: Vec<String> = nodes.iter().map(|n| n["id"].as_str().unwrap().to_string()).collect();
        if !node_ids.is_empty() {
            let ids_str = node_ids.join(",");
            let query = format!("SELECT source_symbol_id, target_symbol_id, kind FROM edges WHERE source_symbol_id IN ({}) AND target_symbol_id IN ({}) LIMIT 200", ids_str, ids_str);
            if let Ok(mut stmt) = conn.prepare(&query) {
                if let Ok(mut rows) = stmt.query([]) {
                    while let Some(row) = rows.next().unwrap_or(None) {
                        let source: Option<i64> = row.get(0).unwrap_or(None);
                        let target: i64 = row.get(1).unwrap_or(0);
                        if let Some(src) = source {
                            edges.push(json!({
                                "source": src.to_string(),
                                "target": target.to_string(),
                                "type": row.get::<_, String>(2).unwrap_or_default(),
                            }));
                        }
                    }
                }
            }
        }
    }

    Json(json!({ "nodes": nodes, "edges": edges }))
}

async fn sse_handler() -> impl axum::response::IntoResponse {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(100);

    tokio::spawn(async move {
        let mut last_count = 0_i64;
        let mut last_file_count = 0_i64;
        
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;
            if let Ok(conn) = rusqlite::Connection::open(".codebroker/codebroker.db") {
                let current_mcp_count: i64 = conn.query_row("SELECT COUNT(*) FROM mcp_analytics_events", [], |r| r.get(0)).unwrap_or(0);
                if current_mcp_count > last_count {
                    last_count = current_mcp_count;
                    let _ = tx.send(Ok(Event::default().event("update").data("mcp_activity"))).await;
                }

                let current_file_count: i64 = conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap_or(0);
                if current_file_count != last_file_count {
                    last_file_count = current_file_count;
                    let _ = tx.send(Ok(Event::default().event("update").data("index_update"))).await;
                }
            }
        }
    });

    Sse::new(ReceiverStream::new(rx)).keep_alive(axum::response::sse::KeepAlive::new())
}

async fn serve_dashboard() -> Html<&'static str> {
    Html(include_str!("../../dashboard/dist/index.html"))
}

pub async fn start_server() {
    let app = Router::new()
        .route("/", get(serve_dashboard))
        .route("/api/v1/stats/overview", get(get_overview))
        .route("/api/v1/stats/health", get(get_health))
        .route("/api/v1/mcp/activity", get(get_mcp_activity))
        .route("/api/v1/mcp/tools", get(get_mcp_tools))
        .route("/api/v1/graph/snapshot", get(get_graph_snapshot))
        .route("/api/v1/events", get(sse_handler));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await.unwrap();
    println!("CodeBroker Analytics Dashboard API running on http://127.0.0.1:3000");
    axum::serve(listener, app).await.unwrap();
}
