#[derive(Debug)]
// main src of truth that plays the major role between parser db and the cli

// retruning the following below
pub struct SymbolNode {
    pub name: String,
    pub kind: String,
    pub line_number: usize
}

pub struct ImportNode {
    pub name: String,
    pub line_number: usize
}