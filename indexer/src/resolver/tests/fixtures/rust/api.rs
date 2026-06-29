use crate::db::{Database, create_db};

pub struct ApiClient {
    pub db: Database,
}

impl ApiClient {
    pub fn new() -> Self {
        Self { db: create_db() }
    }
    
    pub fn fetch_user(&self) -> Vec<()> {
        self.db.query("SELECT * FROM users")
    }
}
