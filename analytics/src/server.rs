use axum::{Json, Router, extract::State, response::Html, routing::get};
use axum::response::sse::{Event, Sse};
use serde_json::{Value, json};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Instant;
use tokio_stream::wrappers::ReceiverStream;

const DB_PATH: &str = ".codebroker/codebroker.db";
const PALETTE: [&str; 6] = ["#ff6b35", "#22c55e", "#3b82f6", "#a855f7", "#ec4899", "#eab308"];

#[derive(Clone)]
struct AppState {
    started_at: Instant,
    port: u16,
}

fn open_db() -> Option<storage::Database> {
    storage::Database::new(DB_PATH).ok()
}

/// Real on-disk byte total for a bounded set of indexed files. Capped so a
/// huge monorepo can't turn a dashboard refresh into tens of thousands of
/// stat() syscalls.
fn real_repo_bytes(db: &storage::Database, cap: usize) -> (u64, usize) {
    let mut stmt = match db.conn.prepare("SELECT path FROM files LIMIT ?1") {
        Ok(s) => s,
        Err(_) => return (0, 0),
    };
    let rows = match stmt.query_map(rusqlite::params![cap as i64], |r| r.get::<_, String>(0)) {
        Ok(r) => r,
        Err(_) => return (0, 0),
    };
    let mut total = 0u64;
    let mut counted = 0usize;
    for path in rows.flatten() {
        let abs = db.resolve_path(&path);
        if let Ok(meta) = std::fs::metadata(&abs) {
            if meta.is_file() {
                total += meta.len();
                counted += 1;
            }
        }
    }
    (total, counted)
}

async fn get_overview() -> Json<Value> {
    let mut files_indexed = 0_i64;
    let mut symbols_indexed = 0_i64;
    let mut relationships_indexed = 0_i64;
    let mut total_tokens_avoided = 0_i64;
    let mut total_tokens_used = 0_i64;
    let mut total_raw_tokens = 0_i64;
    let mut total_calls = 0_i64;
    let mut failed_calls = 0_i64;
    let mut avg_latency_ms = 0_f64;
    let mut communities = 0_i64;
    let mut entrypoints = 0_i64;
    let mut embedded_symbols = 0_i64;
    let mut repository_size_bytes = 0_u64;
    let mut token_usage_graph = Vec::new();
    let mut workspace_name = String::new();
    let mut workspace_path = String::new();

    if let Some(db) = open_db() {
        let conn = &db.conn;
        workspace_path = db.project_root.clone();
        workspace_name = std::path::Path::new(&db.project_root)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| workspace_path.clone());

        files_indexed = conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap_or(0);
        symbols_indexed = conn.query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0)).unwrap_or(0);
        relationships_indexed = conn.query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0)).unwrap_or(0);
        total_tokens_avoided = conn.query_row("SELECT COALESCE(SUM(token_reduction),0) FROM mcp_analytics_events", [], |r| r.get(0)).unwrap_or(0);
        total_tokens_used = conn.query_row("SELECT COALESCE(SUM(delivered_token_count),0) FROM mcp_analytics_events", [], |r| r.get(0)).unwrap_or(0);
        total_raw_tokens = conn.query_row("SELECT COALESCE(SUM(estimated_raw_context_tokens),0) FROM mcp_analytics_events", [], |r| r.get(0)).unwrap_or(0);
        total_calls = conn.query_row("SELECT COUNT(*) FROM mcp_analytics_events", [], |r| r.get(0)).unwrap_or(0);
        failed_calls = conn.query_row("SELECT COUNT(*) FROM mcp_analytics_events WHERE success = 0", [], |r| r.get(0)).unwrap_or(0);
        avg_latency_ms = conn.query_row("SELECT COALESCE(AVG(execution_time_ms),0) FROM mcp_analytics_events", [], |r| r.get(0)).unwrap_or(0.0);
        communities = conn.query_row("SELECT COUNT(DISTINCT community_id) FROM symbol_features", [], |r| r.get(0)).unwrap_or(0);
        entrypoints = conn.query_row("SELECT COUNT(*) FROM entrypoints", [], |r| r.get(0)).unwrap_or(0);
        embedded_symbols = conn.query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get(0)).unwrap_or(0);

        let (bytes, _counted) = real_repo_bytes(&db, 20_000);
        repository_size_bytes = bytes;

        // Chronological hourly buckets (real date+hour, not hour-of-day —
        // grouping by '%H:00' alone silently merges the same hour across
        // different days into one bucket). Only the most recent 24 buckets
        // that actually have data are returned.
        if let Ok(mut stmt) = conn.prepare(
            "SELECT strftime('%Y-%m-%dT%H:00', created_at) as bucket,
                    SUM(delivered_token_count) as used,
                    SUM(token_reduction) as saved
             FROM mcp_analytics_events
             GROUP BY bucket
             ORDER BY bucket DESC
             LIMIT 24"
        ) {
            if let Ok(mut rows) = stmt.query([]) {
                while let Some(row) = rows.next().unwrap_or(None) {
                    let bucket: String = row.get(0).unwrap_or_default();
                    let used: i64 = row.get(1).unwrap_or(0);
                    let saved: i64 = row.get(2).unwrap_or(0);
                    // "MM/DD HH:00" rather than a bare hour: activity spanning
                    // more than a day would otherwise show the same "16:00"
                    // label for two unrelated points on different dates.
                    let label = match bucket.split_once('T') {
                        Some((date, hour)) => {
                            let mut parts = date.split('-');
                            let (_, month, day) = (parts.next(), parts.next(), parts.next());
                            match (month, day) {
                                (Some(m), Some(d)) => format!("{}/{} {}", m, d, hour),
                                _ => bucket.clone(),
                            }
                        }
                        None => bucket.clone(),
                    };
                    token_usage_graph.push(json!({ "time": label, "used": used, "saved": saved }));
                }
            }
        }
        token_usage_graph.reverse();
    }

    let success_rate = if total_calls > 0 {
        ((total_calls - failed_calls) as f64 / total_calls as f64) * 100.0
    } else {
        100.0
    };

    let cost_saved_cents = analytics_cost_cents(total_tokens_avoided.max(0) as usize);

    Json(json!({
        "workspace_name": workspace_name,
        "workspace_path": workspace_path,
        "files_indexed": files_indexed,
        "symbols_indexed": symbols_indexed,
        "relationships_indexed": relationships_indexed,
        "communities": communities,
        "entrypoints": entrypoints,
        "embedded_symbols": embedded_symbols,
        "repository_size_bytes": repository_size_bytes,
        "total_tokens_avoided": total_tokens_avoided,
        "total_tokens_used": total_tokens_used,
        "total_raw_tokens": total_raw_tokens,
        "total_calls": total_calls,
        "failed_calls": failed_calls,
        "success_rate": success_rate,
        "avg_latency_ms": avg_latency_ms,
        "est_cost_saved_cents": cost_saved_cents,
        "token_usage_graph": token_usage_graph,
    }))
}

fn analytics_cost_cents(tokens_saved: usize) -> f64 {
    crate::accounting::CostAccounting::calculate_cents_saved(tokens_saved, "claude")
}

async fn get_health(State(state): State<Arc<AppState>>) -> Json<Value> {
    let mut cache_hits = 0_i64;
    let mut cache_misses = 0_i64;
    let mut db_size_bytes = 0_u64;
    let mut index_freshness_ms = 0_u64;
    let mut stale_files = 0_i64;
    let mut files_checked = 0_i64;
    let mut db_present = false;

    if let Ok(metadata) = std::fs::metadata(DB_PATH) {
        db_present = true;
        db_size_bytes = metadata.len();
        if let Ok(modified) = metadata.modified() {
            if let Ok(duration) = std::time::SystemTime::now().duration_since(modified) {
                index_freshness_ms = duration.as_millis() as u64;
            }
        }
    }

    if let Some(db) = open_db() {
        cache_hits = db.conn.query_row("SELECT COUNT(*) FROM mcp_analytics_events WHERE cache_hit = 1", [], |r| r.get(0)).unwrap_or(0);
        cache_misses = db.conn.query_row("SELECT COUNT(*) FROM mcp_analytics_events WHERE cache_hit = 0", [], |r| r.get(0)).unwrap_or(0);
        if let Ok((stale, checked)) = db.count_stale_files(2000) {
            stale_files = stale as i64;
            files_checked = checked as i64;
        }
    }

    let hit_rate = if cache_hits + cache_misses > 0 {
        (cache_hits as f64 / (cache_hits + cache_misses) as f64) * 100.0
    } else {
        0.0
    };

    let status = if !db_present {
        "not_indexed"
    } else if stale_files > 0 {
        "stale"
    } else {
        "healthy"
    };

    Json(json!({
        "status": status,
        "cache_hit_rate": hit_rate,
        "db_size_bytes": db_size_bytes,
        "index_freshness_ms": index_freshness_ms,
        "stale_files": stale_files,
        "files_checked": files_checked,
        "port": state.port,
        "uptime_ms": state.started_at.elapsed().as_millis(),
        "mcp_server_status": "connected",
    }))
}

async fn get_mcp_activity() -> Json<Value> {
    let mut activities = Vec::new();
    if let Some(db) = open_db() {
        if let Ok(mut stmt) = db.conn.prepare("SELECT tool_name, prompt, success, execution_time_ms, delivered_token_count, token_reduction, cache_hit, created_at FROM mcp_analytics_events ORDER BY id DESC LIMIT 50") {
            if let Ok(mut rows) = stmt.query([]) {
                while let Some(row) = rows.next().unwrap_or(None) {
                    activities.push(json!({
                        "tool": row.get::<_, String>(0).unwrap_or_default(),
                        "prompt": row.get::<_, Option<String>>(1).unwrap_or_default().unwrap_or_default(),
                        "success": row.get::<_, bool>(2).unwrap_or(true),
                        "latency_ms": row.get::<_, i64>(3).unwrap_or(0),
                        "tokens": row.get::<_, i64>(4).unwrap_or(0),
                        "tokens_saved": row.get::<_, i64>(5).unwrap_or(0),
                        "cache_hit": row.get::<_, bool>(6).unwrap_or(false),
                        "timestamp": row.get::<_, String>(7).unwrap_or_default()
                    }));
                }
            }
        }
    }
    Json(json!(activities))
}

async fn get_errors() -> Json<Value> {
    let mut errors = Vec::new();
    if let Some(db) = open_db() {
        if let Ok(mut stmt) = db.conn.prepare(
            "SELECT id, tool_name, prompt, execution_time_ms, created_at
             FROM mcp_analytics_events WHERE success = 0 ORDER BY id DESC LIMIT 50"
        ) {
            if let Ok(mut rows) = stmt.query([]) {
                while let Some(row) = rows.next().unwrap_or(None) {
                    let prompt: String = row.get::<_, Option<String>>(2).unwrap_or_default().unwrap_or_default();
                    let truncated = if prompt.len() > 240 { format!("{}...", &prompt[..240]) } else { prompt };
                    errors.push(json!({
                        "id": row.get::<_, i64>(0).unwrap_or(0),
                        "tool": row.get::<_, String>(1).unwrap_or_default(),
                        "arguments": truncated,
                        "latency_ms": row.get::<_, i64>(3).unwrap_or(0),
                        "timestamp": row.get::<_, String>(4).unwrap_or_default(),
                    }));
                }
            }
        }
    }
    Json(json!(errors))
}

async fn get_mcp_tools() -> Json<Value> {
    let mut tools = Vec::new();
    if let Some(db) = open_db() {
        if let Ok(mut stmt) = db.conn.prepare(
            "SELECT tool_name, COUNT(*), SUM(execution_time_ms), SUM(token_reduction), SUM(delivered_token_count), SUM(CASE WHEN success = 0 THEN 1 ELSE 0 END)
             FROM mcp_analytics_events GROUP BY tool_name ORDER BY COUNT(*) DESC"
        ) {
            if let Ok(mut rows) = stmt.query([]) {
                while let Some(row) = rows.next().unwrap_or(None) {
                    let count: i64 = row.get(1).unwrap_or(0);
                    let sum_time: i64 = row.get(2).unwrap_or(0);
                    let savings: i64 = row.get(3).unwrap_or(0);
                    let used: i64 = row.get(4).unwrap_or(0);
                    let failures: i64 = row.get(5).unwrap_or(0);
                    tools.push(json!({
                        "name": row.get::<_, String>(0).unwrap_or_default(),
                        "calls": count,
                        "avg_latency": if count > 0 { sum_time / count } else { 0 },
                        "tokens_saved": savings,
                        "tokens_used": used,
                        "failures": failures,
                    }));
                }
            }
        }
    }
    Json(json!(tools))
}

async fn get_codebase_overview() -> Json<Value> {
    let Some(db) = open_db() else {
        return Json(json!({ "available": false }));
    };

    let overview = query::engine::build_project_overview(&db).ok();
    let hotspots = query::graph::architectural_hotspots(&db, 8, None).ok();
    let cycles = query::graph::dependency_cycles(&db, 6, None, false).ok();

    let total_symbols: i64 = db.conn.query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0)).unwrap_or(0);
    let total_edges: i64 = db.conn.query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0)).unwrap_or(0);
    let orphan_symbols: i64 = db.conn.query_row(
        "SELECT COUNT(*) FROM symbol_features WHERE fan_in = 0 AND fan_out = 0", [], |r| r.get(0)
    ).unwrap_or(0);

    let orphan_pct = if total_symbols > 0 { (orphan_symbols as f64 / total_symbols as f64) * 100.0 } else { 0.0 };
    let dependency_density = if total_symbols > 0 { total_edges as f64 / total_symbols as f64 } else { 0.0 };
    let cross_file_cycles = cycles.as_ref().map(|c| c.cross_file_cycles_found).unwrap_or(0);

    // Composite health score: cross-file cycles are the architecturally
    // dangerous kind of cycle (same-file mutual recursion is usually
    // benign), so they're penalized harder than plain orphaned symbols.
    // Both penalties are capped so one extreme outlier metric can't drag
    // the score to a meaningless 0.
    let cycle_penalty = (cross_file_cycles as f64 * 3.0).min(40.0);
    let orphan_penalty = (orphan_pct * 0.4).min(30.0);
    let health_score = (100.0 - cycle_penalty - orphan_penalty).clamp(0.0, 100.0);

    let directories: Vec<Value> = overview
        .as_ref()
        .map(|o| {
            o.top_level_directories
                .iter()
                .take(8)
                .map(|d| {
                    let bytes: u64 = if let Ok(mut stmt) = db.conn.prepare(
                        "SELECT path FROM files WHERE path LIKE ?1 LIMIT 5000"
                    ) {
                        let pattern = format!("{}/%", d.path);
                        let rows = stmt.query_map(rusqlite::params![pattern], |r| r.get::<_, String>(0));
                        rows.map(|rows| {
                            rows.flatten()
                                .filter_map(|p| std::fs::metadata(db.resolve_path(&p)).ok().map(|m| m.len()))
                                .sum()
                        }).unwrap_or(0)
                    } else { 0 };
                    json!({
                        "path": d.path,
                        "files": d.file_count,
                        "symbols": d.symbol_count,
                        "bytes": bytes,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let languages: Vec<Value> = overview
        .as_ref()
        .map(|o| {
            let total: i64 = o.languages.values().sum::<i64>().max(1);
            let mut v: Vec<(&String, &i64)> = o.languages.iter().collect();
            v.sort_by(|a, b| b.1.cmp(a.1));
            v.into_iter()
                .take(8)
                .map(|(ext, count)| json!({
                    "extension": ext,
                    "files": count,
                    "percent": (*count as f64 / total as f64) * 100.0,
                }))
                .collect()
        })
        .unwrap_or_default();

    Json(json!({
        "available": true,
        "health_score": health_score.round(),
        "dependency_density": dependency_density,
        "circular_dependencies": cross_file_cycles,
        "orphan_symbols": orphan_symbols,
        "orphan_symbol_percent": orphan_pct,
        "languages": languages,
        "directories": directories,
        "hotspot_files": hotspots.map(|h| h.top_file_hotspots).unwrap_or_default(),
        "cycle_examples": cycles.map(|c| c.cycles).unwrap_or_default(),
    }))
}

async fn get_graph_snapshot() -> Json<Value> {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    if let Some(db) = open_db() {
        let conn = &db.conn;
        // Prefer the precomputed graph features (pagerank/fan_in/fan_out/
        // community_id) — real signals from the same PageRank + label-
        // propagation pass the indexer already runs, so ranking and
        // clustering here cost one query instead of recomputing anything.
        let mut used_features = false;
        if let Ok(mut stmt) = conn.prepare(
            "SELECT s.id, s.name, s.kind, f.path, sf.pagerank, sf.fan_in, sf.fan_out, sf.community_id
             FROM symbols s
             JOIN files f ON s.file_id = f.id
             JOIN symbol_features sf ON sf.symbol_id = s.id
             ORDER BY (sf.fan_in + sf.fan_out) DESC, sf.pagerank DESC
             LIMIT 120"
        ) {
            if let Ok(mut rows) = stmt.query([]) {
                while let Some(row) = rows.next().unwrap_or(None) {
                    used_features = true;
                    let id: i64 = row.get(0).unwrap_or(0);
                    let fan_in: i64 = row.get(5).unwrap_or(0);
                    let fan_out: i64 = row.get(6).unwrap_or(0);
                    let community: i64 = row.get(7).unwrap_or(0);
                    nodes.push(json!({
                        "id": id.to_string(),
                        "label": row.get::<_, String>(1).unwrap_or_default(),
                        "kind": row.get::<_, String>(2).unwrap_or_default(),
                        "file_path": row.get::<_, String>(3).unwrap_or_default(),
                        "pagerank": row.get::<_, f64>(4).unwrap_or(0.0),
                        "connections": fan_in + fan_out,
                        "community": community,
                        "color": PALETTE[(community.unsigned_abs() as usize) % PALETTE.len()],
                    }));
                }
            }
        }

        if !used_features {
            // symbol_features hasn't been populated (older index) — fall
            // back to a live edge-count query instead of showing nothing.
            if let Ok(mut stmt) = conn.prepare(
                "SELECT s.id, s.name, s.kind, f.path, COUNT(e.id) as edge_count
                 FROM symbols s
                 JOIN files f ON s.file_id = f.id
                 LEFT JOIN edges e ON s.id = e.source_symbol_id OR s.id = e.target_symbol_id
                 GROUP BY s.id
                 ORDER BY edge_count DESC
                 LIMIT 120"
            ) {
                if let Ok(mut rows) = stmt.query([]) {
                    while let Some(row) = rows.next().unwrap_or(None) {
                        let id: i64 = row.get(0).unwrap_or(0);
                        let connections: i64 = row.get(4).unwrap_or(0);
                        nodes.push(json!({
                            "id": id.to_string(),
                            "label": row.get::<_, String>(1).unwrap_or_default(),
                            "kind": row.get::<_, String>(2).unwrap_or_default(),
                            "file_path": row.get::<_, String>(3).unwrap_or_default(),
                            "pagerank": 0.0,
                            "connections": connections,
                            "community": 0,
                            "color": PALETTE[0],
                        }));
                    }
                }
            }
        }

        let node_ids: Vec<String> = nodes.iter().filter_map(|n| n["id"].as_str().map(|s| s.to_string())).collect();
        if !node_ids.is_empty() {
            let ids_str = node_ids.join(",");
            let query = format!(
                "SELECT source_symbol_id, target_symbol_id, kind FROM edges WHERE source_symbol_id IN ({}) AND target_symbol_id IN ({}) LIMIT 300",
                ids_str, ids_str
            );
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
            if let Some(db) = open_db() {
                let current_mcp_count: i64 = db.conn.query_row("SELECT COUNT(*) FROM mcp_analytics_events", [], |r| r.get(0)).unwrap_or(0);
                if current_mcp_count > last_count {
                    last_count = current_mcp_count;
                    let _ = tx.send(Ok(Event::default().event("update").data("mcp_activity"))).await;
                }

                let current_file_count: i64 = db.conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap_or(0);
                if current_file_count != last_file_count {
                    last_file_count = current_file_count;
                    let _ = tx.send(Ok(Event::default().event("update").data("index_update"))).await;
                }
            } else if tx.is_closed() {
                break;
            }
        }
    });

    Sse::new(ReceiverStream::new(rx)).keep_alive(axum::response::sse::KeepAlive::new())
}

async fn serve_dashboard() -> Html<&'static str> {
    Html(include_str!("../../dashboard/dist/index.html"))
}

/// Binds the first free port starting at `preferred`, trying up to `preferred + span`
/// before giving up. Returns the bound listener together with the port it landed on.
async fn bind_first_available(preferred: u16, span: u16) -> std::io::Result<(tokio::net::TcpListener, u16)> {
    let mut last_err = None;
    for offset in 0..=span {
        let port = preferred.saturating_add(offset);
        match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
            Ok(listener) => return Ok((listener, port)),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| std::io::Error::new(std::io::ErrorKind::AddrInUse, "no free port found")))
}

pub async fn start_server() {
    let (listener, port) = bind_first_available(3000, 50)
        .await
        .expect("Could not find a free port between 3000 and 3050 to run the CodeBroker dashboard on. Free one up and retry.");

    let state = Arc::new(AppState { started_at: Instant::now(), port });

    let app = Router::new()
        .route("/", get(serve_dashboard))
        .route("/api/v1/stats/overview", get(get_overview))
        .route("/api/v1/stats/health", get(get_health))
        .route("/api/v1/mcp/activity", get(get_mcp_activity))
        .route("/api/v1/mcp/tools", get(get_mcp_tools))
        .route("/api/v1/codebase/overview", get(get_codebase_overview))
        .route("/api/v1/errors", get(get_errors))
        .route("/api/v1/graph/snapshot", get(get_graph_snapshot))
        .route("/api/v1/events", get(sse_handler))
        .with_state(state);

    if port != 3000 {
        println!("Port 3000 was busy — CodeBroker Analytics Dashboard API running on http://127.0.0.1:{port} instead");
    } else {
        println!("CodeBroker Analytics Dashboard API running on http://127.0.0.1:{port}");
    }
    axum::serve(listener, app).await.unwrap();
}
