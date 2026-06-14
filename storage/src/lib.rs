// public api of the storage crate

pub mod schema;
pub mod db;

// reimporting everything so that it could use it as `storage::Database` by the other crates
pub use db::Database;