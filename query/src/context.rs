use serde::{Serialize, Deserialize};
use storage::Database;
use rusqlite::Result;

#[derive(Debug, Serialize, Deserialize)]

pub struct ContextObject {
    pub target_name: String,
    pub target_kind: String,
    pub defining_file: String,
    pub line_number: usize,

    pub reverse_dependencies: Vec<String>, // files that rely on symbols

    pub siblings: Vec<String>, // symbols that are defubed ub tge exact same file

    pub siblings: Vec<String>,
    


}