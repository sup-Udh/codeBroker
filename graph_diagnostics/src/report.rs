use serde::{Deserialize, Serialize};
use crate::traits::{PipelineReport, StageStatus};

impl PipelineReport {
    pub fn to_human_readable(&self) -> String {
        let mut out = String::new();
        out.push_str("====================================\n");
        out.push_str("CodeBroker Pipeline Diagnostics\n");
        out.push_str("====================================\n\n");

        // 1. Summary Header
        for report in &self.stages {
            let status_str = match report.status {
                StageStatus::Pass => "PASS",
                StageStatus::Warning => "WARNING",
                StageStatus::Fail => "FAIL",
                StageStatus::Skipped => "SKIPPED",
            };
            let stage_name = format!("{}", report.stage);
            let dots = ".".repeat(30_usize.saturating_sub(stage_name.len()));
            out.push_str(&format!("{}{} {}\n", stage_name, dots, status_str));
        }

        out.push_str("\n====================================\n");
        out.push_str("Stage Details\n");
        out.push_str("====================================\n\n");

        // 2. Stage Details
        for report in &self.stages {
            if report.status == StageStatus::Skipped {
                continue;
            }

            out.push_str(&format!("--- {} ---\n", report.stage));
            out.push_str(&format!("Performance: {}ms\n", report.execution_time_ms));
            
            if !report.metrics.is_empty() {
                out.push_str("Metrics:\n");
                let mut metrics: Vec<_> = report.metrics.iter().collect();
                // Sort keys alphabetically for stable output
                metrics.sort_by_key(|k| k.0);
                for (k, v) in metrics {
                    out.push_str(&format!("  {}: {}\n", k, v));
                }
            }

            if !report.findings.is_empty() {
                out.push_str("Top Issues:\n");
                for (i, f) in report.findings.iter().take(5).enumerate() {
                    out.push_str(&format!("  {}. [{:?}] {}\n", i + 1, f.severity, f.title));
                    out.push_str(&format!("     {}\n", f.description));
                    out.push_str(&format!("     Likely cause: {}\n", f.likely_cause));
                    out.push_str(&format!("     Suggested fix: {}\n", f.suggested_fix));
                }
                if report.findings.len() > 5 {
                    out.push_str(&format!("  ... and {} more findings\n", report.findings.len() - 5));
                }
            }
            out.push_str("\n");
        }

        out
    }
}
