use crate::response::{ResponseProfile, TokenBudget};
use rusqlite::Result;
use serde::{Deserialize, Serialize};
use storage::Database;

fn contains_call(haystack: &str, needle: &str) -> bool {
    let bytes = haystack.as_bytes();
    let needle_bytes = needle.as_bytes();
    let mut start = 0;
    while let Some(rel_pos) = haystack[start..].find(needle) {
        let pos = start + rel_pos;
        let boundary_ok = pos == 0 || {
            let prev = bytes[pos - 1];
            !(prev.is_ascii_alphanumeric() || prev == b'_')
        };
        let end = pos + needle_bytes.len();
        let after_ok = end >= bytes.len() || {
            let next = bytes[end];
            !(next.is_ascii_alphanumeric() || next == b'_')
        };
        if boundary_ok && after_ok && haystack[end..].trim_start().starts_with('(') {
            return true;
        }
        start = pos + 1;
    }
    false
}

pub struct ContextResponseBuilder<'a> {
    db: &'a Database,
    pub target_name: String,
    pub target_kind: String,
    pub defining_file: String,
    pub line_number: usize,
    pub signature: Option<String>,
    pub directive: Option<String>,
    pub decorators: Vec<String>,
    pub graph_indexed: bool,
    pub stale: bool,
    pub file_id: i64,
    pub symbol_id: Option<i64>,
    pub symbol_name: String,
    pub profile: ResponseProfile,
    pub budget: TokenBudget,
}

impl<'a> ContextResponseBuilder<'a> {
    pub fn new(
        db: &'a Database,
        symbol_name: &str,
        path_scope: Option<&str>,
        profile: ResponseProfile,
    ) -> Result<Option<Self>> {
        let mut target_info = if let Some(scope) = path_scope {
            let mut stmt = db.conn.prepare(
                "SELECT symbols.id, symbols.name, symbols.kind, files.path, symbols.start_line, symbols.file_id, symbols.signature, files.metadata, files.content_hash
                 FROM symbols
                 JOIN files ON symbols.file_id = files.id
                 WHERE symbols.name = ?1 AND files.path LIKE ?2 LIMIT 1"
            )?;
            let like_scope = format!("%{}%", scope);
            stmt.query_row(rusqlite::params![symbol_name, like_scope], |row| {
                Ok((
                    row.get::<_, i64>(0)?,            // id
                    row.get::<_, String>(1)?,         // name
                    row.get::<_, String>(2)?,         // kind
                    row.get::<_, String>(3)?,         // path
                    row.get::<_, i64>(4)?,            // line_number
                    row.get::<_, i64>(5)?,            // file_id
                    row.get::<_, Option<String>>(6)?, // signature
                    row.get::<_, Option<String>>(7)?, // directive
                    row.get::<_, Option<String>>(8)?, // content_hash
                ))
            })
        } else {
            let mut stmt = db.conn.prepare(
                "SELECT symbols.id, symbols.name, symbols.kind, files.path, symbols.start_line, symbols.file_id, symbols.signature, files.metadata, files.content_hash
                 FROM symbols
                 JOIN files ON symbols.file_id = files.id
                 WHERE symbols.name = ?1 LIMIT 1"
            )?;
            stmt.query_row(rusqlite::params![symbol_name], |row| {
                Ok((
                    row.get::<_, i64>(0)?,            // id
                    row.get::<_, String>(1)?,         // name
                    row.get::<_, String>(2)?,         // kind
                    row.get::<_, String>(3)?,         // path
                    row.get::<_, i64>(4)?,            // line_number
                    row.get::<_, i64>(5)?,            // file_id
                    row.get::<_, Option<String>>(6)?, // signature
                    row.get::<_, Option<String>>(7)?, // directive
                    row.get::<_, Option<String>>(8)?, // content_hash
                ))
            })
        };

        let (
            sym_id,
            name,
            kind,
            path,
            line_number,
            file_id,
            signature,
            directive,
            indexed_content_hash,
        ) = match target_info {
            Ok(info) => (
                Some(info.0),
                info.1,
                info.2,
                info.3,
                info.4,
                info.5,
                info.6,
                info.7,
                info.8,
            ),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                let mut file_stmt = db.conn.prepare(
                    "SELECT id, path, metadata, content_hash FROM files WHERE path LIKE ?1 LIMIT 1",
                )?;
                let file_search = format!("%{}%", symbol_name);
                if let Ok((f_id, f_path, f_dir, f_hash)) =
                    file_stmt.query_row(rusqlite::params![file_search], |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, Option<String>>(3)?,
                        ))
                    })
                {
                    (
                        None,
                        symbol_name.to_string(),
                        "file".to_string(),
                        db.resolve_path(&f_path),
                        1,
                        f_id,
                        None,
                        f_dir,
                        f_hash,
                    )
                } else {
                    return Ok(None);
                }
            }
            Err(e) => return Err(e),
        };

        let live_content = std::fs::read(db.resolve_path(&path)).ok();
        let stale = match (&indexed_content_hash, &live_content) {
            (Some(stored_hash), Some(content)) => storage::hash_content(content) != *stored_hash,
            _ => false,
        };

        let mut final_signature = signature.clone();
        let mut decorators = Vec::new();
        if let Some(sig_str) = &signature {
            let parts: Vec<&str> = sig_str.split_whitespace().collect();
            let mut cleaned_parts = Vec::new();
            for part in parts {
                if part.starts_with('@') {
                    decorators.push(part.to_string());
                } else {
                    cleaned_parts.push(part);
                }
            }
            if cleaned_parts.is_empty() {
                final_signature = None;
            } else {
                final_signature = Some(cleaned_parts.join(" "));
            }
        }

        let total_edges: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))
            .unwrap_or(0);
        let graph_indexed = total_edges > 0;

        Ok(Some(Self {
            db,
            target_name: name.clone(),
            target_kind: kind,
            defining_file: db.resolve_path(&path),
            line_number: line_number as usize,
            signature: final_signature,
            directive,
            decorators,
            graph_indexed,
            stale,
            file_id,
            symbol_id: sym_id,
            symbol_name: name.clone(),
            budget: TokenBudget::new(profile),
            profile,
        }))
    }

    pub fn fetch_reverse_dependencies(&self) -> Result<Vec<String>> {
        if let Some(id) = self.symbol_id {
            let mut deps = crate::graph::get_incoming_edges(
                self.db,
                id,
                Some(crate::graph::CANONICAL_DEPENDENCY_EDGES),
            )?;
            deps.extend(crate::graph::get_incoming_edges_min_confidence(
                self.db,
                id,
                crate::graph::RECEIVER_RESOLVED_EDGES,
                crate::graph::MIN_CONFIDENCE_FOR_RECEIVER_EDGES,
            )?);
            deps.sort();
            deps.dedup();
            Ok(deps)
        } else {
            Ok(Vec::new())
        }
    }

    /// Calls and receiver-resolved method calls (see `RECEIVER_RESOLVED_EDGES`)
    /// pointing at `id`, merged and deduplicated.
    fn fetch_callers_ids(&self, id: i64) -> Result<Vec<String>> {
        let mut callers = crate::graph::get_incoming_edges(self.db, id, Some(&["calls"]))?;
        callers.extend(crate::graph::get_incoming_edges_min_confidence(
            self.db,
            id,
            &["method_call"],
            crate::graph::MIN_CONFIDENCE_FOR_RECEIVER_EDGES,
        )?);
        callers.sort();
        callers.dedup();
        Ok(callers)
    }

    pub fn fetch_same_file_callers(&self) -> Result<Vec<String>> {
        if let Some(id) = self.symbol_id {
            let callers = self.fetch_callers_ids(id)?;
            let mut same_file = Vec::new();
            for c in callers {
                if let Ok(fid) = self.db.conn.query_row(
                    "SELECT file_id FROM symbols WHERE name = ?1 LIMIT 1",
                    rusqlite::params![c],
                    |r| r.get::<_, i64>(0),
                ) {
                    if fid == self.file_id {
                        same_file.push(c);
                    }
                }
            }
            Ok(same_file)
        } else {
            Ok(Vec::new())
        }
    }

    pub fn fetch_siblings(&self) -> Result<Vec<String>> {
        let mut siblings_stmt = self
            .db
            .conn
            .prepare("SELECT name FROM symbols WHERE file_id = ?1 AND name != ?2")?;
        let mut siblings_rows =
            siblings_stmt.query(rusqlite::params![self.file_id, self.symbol_name])?;
        let mut siblings = Vec::new();
        while let Some(row) = siblings_rows.next()? {
            siblings.push(row.get::<_, String>(0)?);
        }
        siblings.sort();
        siblings.dedup();
        Ok(siblings)
    }

    pub fn fetch_forward_dependencies(&self) -> Result<Vec<String>> {
        if let Some(id) = self.symbol_id {
            let mut deps = crate::graph::get_outgoing_edges(
                self.db,
                id,
                Some(crate::graph::CANONICAL_DEPENDENCY_EDGES),
            )?;
            deps.extend(crate::graph::get_outgoing_edges_min_confidence(
                self.db,
                id,
                crate::graph::RECEIVER_RESOLVED_EDGES,
                crate::graph::MIN_CONFIDENCE_FOR_RECEIVER_EDGES,
            )?);
            deps.sort();
            deps.dedup();
            Ok(deps)
        } else {
            Ok(Vec::new())
        }
    }

    pub fn fetch_external_imports(&self) -> Result<Vec<String>> {
        let mut ext_stmt = self
            .db
            .conn
            .prepare("SELECT name FROM relationships WHERE file_id = ?1 AND is_local = 0")?;
        let mut ext_rows = ext_stmt.query(rusqlite::params![self.file_id])?;
        let mut external_imports = Vec::new();
        while let Some(row) = ext_rows.next()? {
            external_imports.push(row.get::<_, String>(0)?);
        }
        external_imports.sort();
        external_imports.dedup();
        Ok(external_imports)
    }

    pub fn fetch_callers(&self) -> Result<Vec<String>> {
        if let Some(id) = self.symbol_id {
            self.fetch_callers_ids(id)
        } else {
            Ok(Vec::new())
        }
    }

    pub fn fetch_callees(&self) -> Result<Vec<String>> {
        if let Some(id) = self.symbol_id {
            let mut callees = crate::graph::get_outgoing_edges(self.db, id, Some(&["calls"]))?;
            callees.extend(crate::graph::get_outgoing_edges_min_confidence(
                self.db,
                id,
                &["method_call"],
                crate::graph::MIN_CONFIDENCE_FOR_RECEIVER_EDGES,
            )?);
            callees.sort();
            callees.dedup();
            Ok(callees)
        } else {
            Ok(Vec::new())
        }
    }

    pub fn fetch_prop_interfaces(&self) -> Result<Vec<crate::retrieval::SymbolSourceResult>> {
        let mut props = Vec::new();
        let mut stmt = self.db.conn.prepare(
            "SELECT name FROM symbols WHERE file_id = ?1 AND (name LIKE ?2 OR name LIKE ?3)",
        )?;
        let like_props = format!("{}Props", self.symbol_name);
        let like_props_lower = format!("{}props", self.symbol_name.to_lowercase());
        let mut rows = stmt.query(rusqlite::params![
            self.file_id,
            like_props,
            like_props_lower
        ])?;
        while let Some(row) = rows.next()? {
            let p_name: String = row.get(0)?;
            if let Ok(srcs) = crate::retrieval::read_symbol_source_scoped(
                self.db,
                &p_name,
                false,
                Some(&self.defining_file),
            ) {
                props.extend(srcs);
            }
        }
        Ok(props)
    }

    pub fn fetch_wrapped_by(&self) -> Result<Vec<String>> {
        if let Some(id) = self.symbol_id {
            crate::graph::get_incoming_edges(self.db, id, Some(&["wraps_route"]))
        } else {
            Ok(Vec::new())
        }
    }

    pub fn fetch_renders_components(&self) -> Result<Vec<String>> {
        if let Some(id) = self.symbol_id {
            crate::graph::get_outgoing_edges(self.db, id, Some(&["renders_component"]))
        } else {
            Ok(Vec::new())
        }
    }

    pub fn fetch_consumes_hooks(&self) -> Result<Vec<String>> {
        let mut consumes = if let Some(id) = self.symbol_id {
            crate::graph::get_outgoing_edges(self.db, id, Some(&["consumes_hook"]))?
        } else {
            Vec::new()
        };

        let mut raw_hook_stmt = self.db.conn.prepare(
            "SELECT name FROM relationships WHERE file_id = ?1 AND kind = 'consumes_hook'",
        )?;
        let mut raw_hook_rows = raw_hook_stmt.query(rusqlite::params![self.file_id])?;
        while let Some(row) = raw_hook_rows.next()? {
            let hook_name: String = row.get(0)?;
            if !consumes.contains(&hook_name) {
                consumes.push(hook_name);
            }
        }
        consumes.sort();
        consumes.dedup();
        Ok(consumes)
    }

    pub fn build_json(&self) -> Result<serde_json::Value> {
        let mut map = serde_json::Map::new();
        map.insert(
            "target_name".to_string(),
            serde_json::json!(self.target_name),
        );
        map.insert(
            "target_kind".to_string(),
            serde_json::json!(self.target_kind),
        );
        map.insert(
            "defining_file".to_string(),
            serde_json::json!(self.defining_file),
        );
        map.insert(
            "line_number".to_string(),
            serde_json::json!(self.line_number),
        );
        if self.profile != ResponseProfile::Compact {
            if let Some(sig) = &self.signature {
                map.insert("signature".to_string(), serde_json::json!(sig));
            }
            if let Some(dir) = &self.directive {
                map.insert("directive".to_string(), serde_json::json!(dir));
            }
            if !self.decorators.is_empty() {
                map.insert("decorators".to_string(), serde_json::json!(self.decorators));
            }
        }

        let mut b = self.budget.clone();

        let insert_vec = |map: &mut serde_json::Map<String, serde_json::Value>,
                          key: &str,
                          items: Vec<String>,
                          b: &mut TokenBudget| {
            if items.is_empty() {
                return;
            }
            if !b.has_budget() {
                return;
            }
            let mut keep = Vec::new();
            for item in items {
                if b.consume(1) {
                    keep.push(item);
                } else {
                    break;
                }
            }
            if !keep.is_empty() {
                map.insert(key.to_string(), serde_json::json!(keep));
            }
        };

        if self.profile == ResponseProfile::Verbose || self.profile == ResponseProfile::Standard {
            if let Ok(deps) = self.fetch_reverse_dependencies() {
                insert_vec(&mut map, "reverse_dependencies", deps, &mut b);
            }
            if let Ok(deps) = self.fetch_forward_dependencies() {
                insert_vec(&mut map, "forward_dependencies", deps, &mut b);
            }
            if let Ok(callers) = self.fetch_callers() {
                insert_vec(&mut map, "callers", callers, &mut b);
            }
            if let Ok(callees) = self.fetch_callees() {
                insert_vec(&mut map, "callees", callees, &mut b);
            }
            if let Ok(same) = self.fetch_same_file_callers() {
                insert_vec(&mut map, "same_file_callers", same, &mut b);
            }
        }

        if self.profile == ResponseProfile::Verbose {
            if let Ok(ext) = self.fetch_external_imports() {
                insert_vec(&mut map, "external_imports", ext, &mut b);
            }
            if let Ok(sib) = self.fetch_siblings() {
                insert_vec(&mut map, "siblings", sib, &mut b);
            }
            if let Ok(wrap) = self.fetch_wrapped_by() {
                insert_vec(&mut map, "wrapped_by", wrap, &mut b);
            }
            if let Ok(ren) = self.fetch_renders_components() {
                insert_vec(&mut map, "renders_components", ren, &mut b);
            }
            if let Ok(hooks) = self.fetch_consumes_hooks() {
                insert_vec(&mut map, "consumes_hooks", hooks, &mut b);
            }

            if let Ok(props) = self.fetch_prop_interfaces() {
                if !props.is_empty() && b.consume(props.len() * 5) {
                    map.insert("prop_interfaces".to_string(), serde_json::json!(props));
                }
            }
        }

        map.insert(
            "graph_indexed".to_string(),
            serde_json::json!(self.graph_indexed),
        );
        map.insert("stale".to_string(), serde_json::json!(self.stale));

        Ok(serde_json::Value::Object(map))
    }

    pub fn build_markdown(&self) -> Result<String> {
        let mut md = format!(
            "### Symbol: `{}` ({})\n**File:** `{}`\n",
            self.target_name, self.target_kind, self.defining_file
        );

        let mut b = self.budget.clone();

        if self.profile != ResponseProfile::Compact {
            if let Some(sig) = &self.signature {
                md.push_str(&format!("\n**Signature:**\n```\n{}\n```\n", sig));
            }
            if let Some(dir) = &self.directive {
                md.push_str(&format!("**Directive:** `{}`\n", dir));
            }
            if !self.decorators.is_empty() {
                md.push_str(&format!(
                    "**Decorators:** `{}`\n",
                    self.decorators.join(", ")
                ));
            }
        }

        let mut push_list =
            |md: &mut String, title: &str, items: Vec<String>, b: &mut TokenBudget| {
                if items.is_empty() {
                    return;
                }
                if !b.has_budget() {
                    return;
                }
                let mut keep = Vec::new();
                let total = items.len();
                for item in items {
                    if b.consume(1) {
                        keep.push(item);
                    } else {
                        break;
                    }
                }
                if !keep.is_empty() {
                    md.push_str(&format!("\n#### {}\n", title));
                    for k in &keep {
                        md.push_str(&format!("- {}\n", k));
                    }
                    if keep.len() < total {
                        md.push_str(&format!("- *...and {} more*\n", total - keep.len()));
                    }
                }
            };

        if self.profile == ResponseProfile::Verbose || self.profile == ResponseProfile::Standard {
            if let Ok(callers) = self.fetch_callers() {
                push_list(&mut md, "Callers", callers, &mut b);
            }
            if let Ok(callees) = self.fetch_callees() {
                push_list(&mut md, "Callees", callees, &mut b);
            }
            if let Ok(same) = self.fetch_same_file_callers() {
                push_list(&mut md, "Same-File Callers", same, &mut b);
            }
            if let Ok(rev) = self.fetch_reverse_dependencies() {
                push_list(&mut md, "Reverse Dependencies (Importers)", rev, &mut b);
            }
            if let Ok(fwd) = self.fetch_forward_dependencies() {
                push_list(&mut md, "Forward Dependencies (Imports)", fwd, &mut b);
            }
        }

        if self.profile == ResponseProfile::Verbose {
            if let Ok(wrap) = self.fetch_wrapped_by() {
                push_list(&mut md, "Wrapped By", wrap, &mut b);
            }
            if let Ok(ren) = self.fetch_renders_components() {
                push_list(&mut md, "Renders Components", ren, &mut b);
            }
            if let Ok(hooks) = self.fetch_consumes_hooks() {
                push_list(&mut md, "Consumes Hooks", hooks, &mut b);
            }
            if let Ok(ext) = self.fetch_external_imports() {
                push_list(&mut md, "External Imports", ext, &mut b);
            }
            if let Ok(sib) = self.fetch_siblings() {
                push_list(&mut md, "Siblings", sib, &mut b);
            }

            if let Ok(props) = self.fetch_prop_interfaces() {
                if !props.is_empty() && b.consume(props.len() * 5) {
                    md.push_str("\n#### Prop Interfaces\n");
                    for p in props {
                        md.push_str(&format!(
                            "- `{}` ({} lines)\n",
                            p.symbol_name,
                            p.end_line.saturating_sub(p.start_line) + 1
                        ));
                    }
                }
            }
        }

        Ok(md)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph;

    /// Builds a throwaway project at a temp dir with `route.ts`'s exact
    /// content, indexes its two symbols by hand (no parser dependency needed
    /// — the byte ranges are computed directly off the known fixture text),
    /// and returns an open `Database` pointed at it.
    fn setup_fixture() -> (Database, std::path::PathBuf) {
        let unique = format!(
            "codebroker_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let project_root = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&project_root).unwrap();
        std::fs::create_dir_all(project_root.join(".codebroker")).unwrap();

        let source = "function generateRoomId(): string {\n  return \"x\";\n}\n\nexport async function GET(request: Request) {\n  const id = generateRoomId();\n  return id;\n}\n";
        std::fs::write(project_root.join("route.ts"), source).unwrap();

        let db_path = project_root.join(".codebroker").join("codebroker.db");
        let db = Database::new(db_path.to_str().unwrap()).unwrap();
        db.init_schema().unwrap();

        let content_hash = storage::hash_content(source.as_bytes());
        let file_id = db.insert_file("route.ts", &content_hash).unwrap();

        let gen_id = db
            .insert_symbol(
                file_id,
                &::graph::SymbolNode {
                    name: "generateRoomId".to_string(),
                    kind: "function".to_string(),
                    start_line: 1,
                    end_line: 3,
                    start_byte: 0,
                    end_byte: 46,
                    signature: None,
                    attributes: Vec::new(),
                    metadata: None,
                },
            )
            .unwrap();

        let get_id = db
            .insert_symbol(
                file_id,
                &::graph::SymbolNode {
                    name: "GET".to_string(),
                    kind: "function".to_string(),
                    start_line: 5,
                    end_line: 8,
                    start_byte: 0,
                    end_byte: 145,
                    signature: None,
                    attributes: Vec::new(),
                    metadata: None,
                },
            )
            .unwrap();

        db.insert_edge_attributed(file_id, Some(get_id), gen_id, "calls")
            .unwrap();

        (db, project_root)
    }

    #[test]
    fn same_file_caller_is_not_reported_as_dead_code() {
        let (db, project_root) = setup_fixture();

        let context =
            ContextResponseBuilder::new(&db, "generateRoomId", None, ResponseProfile::Verbose)
                .unwrap()
                .unwrap();

        assert!(
            context
                .fetch_reverse_dependencies()
                .unwrap()
                .contains(&"GET".to_string()),
            "reverse_dependencies now uses canonical edges, so it includes same-file calls"
        );
        assert_eq!(
            context.fetch_same_file_callers().unwrap(),
            vec!["GET".to_string()],
            "GET calls generateRoomId() in the same file"
        );

        std::fs::remove_dir_all(&project_root).ok();
    }

    #[test]
    fn unreferenced_symbol_has_no_same_file_callers() {
        let (db, project_root) = setup_fixture();

        let context = ContextResponseBuilder::new(&db, "GET", None, ResponseProfile::Verbose)
            .unwrap()
            .unwrap();

        assert!(
            context.fetch_same_file_callers().unwrap().is_empty(),
            "nothing in this fixture calls GET"
        );

        std::fs::remove_dir_all(&project_root).ok();
    }
}
