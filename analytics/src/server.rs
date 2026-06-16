// overview REST end point lives here:

use axum::{routing::get, Router, Json, response::Html};
use serde_json::{json, Value};

async fn get_overview() -> Json<Value> {
    let mut total_tokens_avoided = 0_i64;
    let mut total_context_tokens_used = 0_i64;
    let mut total_cost_saved_cents = 0.0_f64;
    let mut cache_hits = 0_i64;
    let mut cache_misses = 0_i64;
    let mut total_llm_calls_avoided = 0_i64;
    let mut get_context_calls = 0_i64;
    let mut impact_analysis_calls = 0_i64;

    // Connect to the real SQLite database
    if let Ok(conn) = rusqlite::Connection::open("codebroker.db") {
        total_tokens_avoided = conn.query_row("SELECT SUM(raw_tokens_avoided) FROM token_metrics", [], |row| row.get(0)).unwrap_or(0);
        total_context_tokens_used = conn.query_row("SELECT SUM(context_tokens_used) FROM token_metrics", [], |row| row.get(0)).unwrap_or(0);
        total_cost_saved_cents = conn.query_row("SELECT SUM(cost_saved_cents) FROM token_metrics", [], |row| row.get(0)).unwrap_or(0.0);
        cache_hits = conn.query_row("SELECT COUNT(*) FROM cache_metrics WHERE status = 'hit'", [], |row| row.get(0)).unwrap_or(0);
        cache_misses = conn.query_row("SELECT COUNT(*) FROM cache_metrics WHERE status = 'miss'", [], |row| row.get(0)).unwrap_or(0);
        total_llm_calls_avoided = conn.query_row("SELECT SUM(hit_count) FROM semantic_summaries", [], |row| row.get(0)).unwrap_or(0);
        
        get_context_calls = conn.query_row("SELECT COUNT(*) FROM analytics_events WHERE event_type = 'get_context'", [], |row| row.get(0)).unwrap_or(0);
        impact_analysis_calls = conn.query_row("SELECT COUNT(*) FROM analytics_events WHERE event_type = 'impact_analysis'", [], |row| row.get(0)).unwrap_or(0);
    }

    let hit_rate = if cache_hits + cache_misses > 0 {
        format!("{:.1}%", (cache_hits as f64 / (cache_hits + cache_misses) as f64) * 100.0)
    } else {
        "0%".to_string()
    };

    Json(json!({
        "total_tokens_avoided": total_tokens_avoided,
        "total_context_tokens_used": total_context_tokens_used,
        "total_cost_saved_cents": total_cost_saved_cents,
        "global_cache_hit_rate": hit_rate,
        "total_llm_calls_avoided": total_llm_calls_avoided,
        "mcp_usage": {
            "get_context": get_context_calls,
            "impact_analysis": impact_analysis_calls
        }
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
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await.unwrap();
    println!("CodeBroker Analytics Dashboard API running on http://127.0.0.1:3000");
    
    // 3. Start the server
    axum::serve(listener, app).await.unwrap();
}