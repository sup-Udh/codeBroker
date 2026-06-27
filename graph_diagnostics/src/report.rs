use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::traits::{DiagnosticFinding, Severity};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphHealth {
    pub score: f64,
    pub metrics: HashMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticsReport {
    pub total_files: i64,
    pub total_symbols: i64,
    pub total_edges: i64,
    pub total_raw_imports: i64,
    
    pub findings: Vec<DiagnosticFinding>,
    pub health: GraphHealth,
    
    pub passed: bool,
}

impl DiagnosticsReport {
    pub fn to_human_readable(&self) -> String {
        let mut out = String::new();
        out.push_str("====================================\n");
        out.push_str("CodeBroker Graph Diagnostics\n");
        out.push_str("====================================\n\n");
        
        out.push_str(&format!("Files   {}\n", self.total_files));
        out.push_str(&format!("Symbols {}\n", self.total_symbols));
        out.push_str(&format!("Edges   {}\n", self.total_edges));
        out.push_str("------------------------------------\n\n");
        
        // Group findings by severity
        let mut criticals = 0;
        let mut errors = 0;
        let mut warnings = 0;
        let mut infos = 0;
        
        for f in &self.findings {
            match f.severity {
                Severity::Critical => criticals += 1,
                Severity::Error => errors += 1,
                Severity::Warning => warnings += 1,
                Severity::Info => infos += 1,
            }
        }
        
        out.push_str("Findings Summary:\n");
        out.push_str(&format!("Critical: {}\n", criticals));
        out.push_str(&format!("Error:    {}\n", errors));
        out.push_str(&format!("Warning:  {}\n", warnings));
        out.push_str(&format!("Info:     {}\n", infos));
        out.push_str("------------------------------------\n\n");
        
        if !self.findings.is_empty() {
            out.push_str("Detailed Findings (Top 10):\n");
            for (i, f) in self.findings.iter().take(10).enumerate() {
                out.push_str(&format!("{}. [{:?}] {}\n", i + 1, f.severity, f.title));
                out.push_str(&format!("   {}\n", f.description));
                out.push_str(&format!("   Likely cause: {}\n", f.likely_cause));
                out.push_str(&format!("   Suggested fix: {}\n\n", f.suggested_fix));
            }
            if self.findings.len() > 10 {
                out.push_str(&format!("... and {} more findings\n\n", self.findings.len() - 10));
            }
        }
        
        out.push_str("Health Metrics:\n");
        let mut metrics: Vec<_> = self.health.metrics.iter().collect();
        metrics.sort_by_key(|k| k.0);
        for (k, v) in metrics {
            out.push_str(&format!("{}: {:.2}\n", k, v));
        }
        
        out.push_str("------------------------------------\n\n");
        let pass_str = if self.passed { "PASS" } else { "FAIL" };
        out.push_str(&format!("Validation: {}\n", pass_str));
        out.push_str(&format!("Overall Graph Health: {:.1}%\n", self.health.score * 100.0));
        out.push_str("====================================\n");
        
        out
    }
}
