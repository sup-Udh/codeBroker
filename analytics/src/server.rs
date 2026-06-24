// overview REST end point lives here:

use axum::{Json, Router, response::Html, routing::get};
use serde_json::{Value, json};

use std::collections::HashMap;

async fn get_overview() -> Json<Value> {
    let mut total_tokens_avoided = 0_i64;
    let mut total_context_tokens_used = 0_i64;
    let mut total_cost_saved_cents = 0.0_f64;
    let mut cache_hits = 0_i64;
    let mut cache_misses = 0_i64;
    let mut mcp_usage: HashMap<String, i64> = HashMap::new();

    if let Ok(conn) = rusqlite::Connection::open(".codebroker/codebroker.db") {
        total_tokens_avoided = conn
            .query_row(
                "SELECT SUM(token_reduction) FROM mcp_analytics_events",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        total_context_tokens_used = conn
            .query_row(
                "SELECT SUM(delivered_token_count) FROM mcp_analytics_events",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        cache_hits = conn
            .query_row(
                "SELECT COUNT(*) FROM mcp_analytics_events WHERE cache_hit = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        cache_misses = conn
            .query_row(
                "SELECT COUNT(*) FROM mcp_analytics_events WHERE cache_hit = 0",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if let Ok(mut stmt) =
            conn.prepare("SELECT tool_name, COUNT(*) FROM mcp_analytics_events GROUP BY tool_name")
        {
            if let Ok(mut rows) = stmt.query([]) {
                while let Some(row) = rows.next().unwrap_or(None) {
                    let tool: String = row.get(0).unwrap_or_default();
                    let count: i64 = row.get(1).unwrap_or(0);
                    mcp_usage.insert(tool, count);
                }
            }
        }

        total_cost_saved_cents = (total_tokens_avoided as f64 / 1_000_000.0) * 300.0;
    }

    let hit_rate = if cache_hits + cache_misses > 0 {
        format!(
            "{:.1}%",
            (cache_hits as f64 / (cache_hits + cache_misses) as f64) * 100.0
        )
    } else {
        "0%".to_string()
    };

    Json(json!({
        "total_tokens_avoided": total_tokens_avoided,
        "total_context_tokens_used": total_context_tokens_used,
        "total_cost_saved_cents": total_cost_saved_cents,
        "global_cache_hit_rate": hit_rate,
        "total_llm_calls_avoided": cache_hits,
        "mcp_usage": mcp_usage
    }))
}

// Add this new route handler!
async fn serve_dashboard() -> Html<&'static str> {
    Html(include_str!("../../dashboard/index.html"))
}

pub async fn start_server() {
    // 1. Build the router
    let app = Router::new()
        .route("/", get(serve_dashboard))
        .route("/api/v1/stats/overview", get(get_overview));

    // 2. Bind the TCP listener
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    println!("CodeBroker Analytics Dashboard API running on http://127.0.0.1:3000");

    // 3. Start the server
    axum::serve(listener, app).await.unwrap();
}
