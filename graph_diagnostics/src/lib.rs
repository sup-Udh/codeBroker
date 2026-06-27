pub mod traits;
pub mod engine;
pub mod report;
pub mod validators;

use storage::Database;
use rusqlite::Result;
use report::DiagnosticsReport;
use engine::DiagnosticsEngine;
use validators::symbols::SymbolValidator;
use validators::edges::EdgeValidator;
use validators::imports::ImportValidator;
use validators::calls::CallValidator;

/// Main entrypoint to run the diagnostics suite against a given database.
pub fn run_diagnostics(db: &Database) -> Result<DiagnosticsReport> {
    let mut engine = DiagnosticsEngine::new();
    
    // Register all default validators
    engine.register_validator(Box::new(SymbolValidator));
    engine.register_validator(Box::new(EdgeValidator));
    engine.register_validator(Box::new(ImportValidator));
    engine.register_validator(Box::new(CallValidator));
    
    engine.run(db)
}
