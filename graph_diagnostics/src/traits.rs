use rusqlite::Result;
use serde::{Deserialize, Serialize};
use storage::Database;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Severity {
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticFinding {
    pub severity: Severity,
    pub title: String,
    pub description: String,
    pub likely_cause: String,
    pub suggested_fix: String,
    pub file_id: Option<i64>,
    pub symbol_id: Option<i64>,
}

pub trait GraphValidator {
    /// Returns the name of the validator.
    fn name(&self) -> &'static str;
    
    /// Runs the validation against the database and returns a list of findings.
    fn validate(&self, db: &Database) -> Result<Vec<DiagnosticFinding>>;
}
