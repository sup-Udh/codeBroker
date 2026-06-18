// main src of truth that plays the major role between parser db and the cli

// retruning the following below
#[derive(Debug, Default)]
pub struct FileMetadata {
    pub directive: Option<String>,
    pub route_path: Option<String>,
    pub route_segment: Option<String>,
}

pub struct SymbolNode {
    pub name: String,
    pub kind: String,
    pub prop_type: Option<String>,
    pub start_line: usize,
    pub end_line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
}

pub struct ImportNode {
    pub name: String,
    pub source: Option<String>,
    pub line_number: usize,
    pub kind: Option<String>, // e.g., "imports", "renders_component", "consumes_hook"
}