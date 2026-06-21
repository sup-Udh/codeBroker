use crate::frontend::LanguageFrontend;
use graph::models::{FileMetadata, ImportNode, SymbolNode};
use crate::typescript_frontend::TsxFrontend;
use crate::javascript_frontend::JavaScriptFrontend;

pub struct VueFrontend;

impl LanguageFrontend for VueFrontend {
    fn can_handle(&self, path: &str) -> bool {
        path.ends_with(".vue")
    }

    fn parse_and_extract(&self, source_code: &str, path: &str) -> Option<(FileMetadata, Vec<SymbolNode>, Vec<ImportNode>)> {
        // Find <script> block and extract its content
        let mut script_content = String::new();
        let mut is_ts = false;
        let mut in_script = false;

        for line in source_code.lines() {
            if line.contains("<script") {
                in_script = true;
                if line.contains("lang=\"ts\"") || line.contains("lang='ts'") {
                    is_ts = true;
                }
                // Maintain line numbers by pushing blank lines
                script_content.push_str("\n");
                continue;
            } else if line.contains("</script>") {
                in_script = false;
                script_content.push_str("\n");
                continue;
            }

            if in_script {
                script_content.push_str(line);
                script_content.push_str("\n");
            } else {
                script_content.push_str("\n");
            }
        }

        let mut result = if is_ts {
            TsxFrontend.parse_and_extract(&script_content, path)
        } else {
            JavaScriptFrontend.parse_and_extract(&script_content, path)
        };

        if let Some((_, _, ref mut imports)) = result {
            // Process template bindings
            let re = regex::Regex::new(r#"(?:@|v-on:|v-model|:)[a-zA-Z0-9_\-]+="([a-zA-Z0-9_]+)(?:\(|")"#).unwrap();
            for (line_idx, line) in source_code.lines().enumerate() {
                for cap in re.captures_iter(line) {
                    if let Some(handler) = cap.get(1) {
                        imports.push(ImportNode {
                            name: handler.as_str().to_string(),
                            source: None,
                            line_number: line_idx + 1,
                            kind: Some("calls".to_string()),
                        });
                    }
                }
            }
        }
        
        result
    }
}
