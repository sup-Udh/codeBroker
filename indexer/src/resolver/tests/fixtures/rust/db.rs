pub struct Database;

impl Database {
    pub fn query(&self, sql: &str) -> Vec<()> {
        vec![]
    }
}

pub fn create_db() -> Database {
    Database
}
