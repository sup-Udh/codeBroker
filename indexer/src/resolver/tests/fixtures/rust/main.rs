mod api;
mod db;

use api::ApiClient;
pub use api::ApiClient as Client;

struct Config {
    api: ApiClient,
}

fn wrap<T>(val: T) -> T {
    val
}

fn main() {
    // Constructors
    let client = ApiClient::new();
    client.fetch_user();
    
    // Destructuring
    let ApiClient { db } = ApiClient::new();
    db.query("...");
    
    // Nested chains
    let config = Config { api: ApiClient::new() };
    config.api.db.query("...");
    
    // Aliases
    let a = &config;
    let b = a;
    b.api.fetch_user();
    
    // Factory functions & Generics
    let wrapped = wrap(ApiClient::new());
    wrapped.fetch_user();
    
    // Recursive aliases
    let mut x = ApiClient::new();
    let mut y = x;
    x = y;
    x.fetch_user();
}
