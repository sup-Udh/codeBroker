// overview REST end point lives here:

use axum::{routing::get, Router, Json};
use serde_json::{json, Value};

async fn get_overview() -> Json<Value> {
    // Returning mock JSON data representing what our SQLite aggregations will eventually output
    Json(json!({
        "total_tokens_avoided": 1450000,
        "total_cost_saved_cents": 435.0,
        "global_cache_hit_rate": "89%",
        "total_llm_calls_avoided": 128
    }))
}

pub async fn start_server() {
    // 1. Build the router
    let app = Router::new()
        .route("/api/v1/stats/overview", get(get_overview));

    // 2. Bind the TCP listener
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await.unwrap();
    println!("CodeBroker Analytics Dashboard API running on http://127.0.0.1:3000");
    
    // 3. Start the server
    axum::serve(listener, app).await.unwrap();
}