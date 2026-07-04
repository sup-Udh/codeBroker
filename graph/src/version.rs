use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphVersion {
    pub index_timestamp: String,
    pub repository_hash: String,
    pub schema_version: String,
    pub indexer_version: String,
    pub parser_version: String,
}

impl GraphVersion {
    pub fn new(
        index_timestamp: String,
        repository_hash: String,
        schema_version: String,
        indexer_version: String,
        parser_version: String,
    ) -> Self {
        Self {
            index_timestamp,
            repository_hash,
            schema_version,
            indexer_version,
            parser_version,
        }
    }
}
