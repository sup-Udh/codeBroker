// public api of the storage crate

pub mod db;
pub mod schema;

// reimporting everything so that it could use it as `storage::Database` by the other crates
pub use db::Database;
pub use db::GENERIC_SYMBOL_NAMES;
pub use db::hash_content;
pub use db::{blob_to_embedding, cosine_similarity, embedding_to_blob};
