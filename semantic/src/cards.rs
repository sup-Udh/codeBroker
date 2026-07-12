use sha2::{Digest, Sha256};

/// Rough token budget for the body portion of a card, in bytes (~4 bytes per
/// token). Embedding whole large bodies dilutes the vector with
/// implementation noise; the head of the body plus name/signature/docs is
/// what lets a conceptual query land near the right symbol.
const BODY_BYTE_BUDGET: usize = 1600;

/// The text actually embedded for one symbol. Assembled from the symbol's
/// identity (relative path, kind, name), its signature, the doc comment
/// immediately above it if any, and the head of its body. This is also the
/// text whose hash keys incremental re-embedding: any change to what would
/// be embedded (including a path or signature change) re-embeds the symbol,
/// and nothing else does.
pub fn build_card(
    rel_path: &str,
    kind: &str,
    name: &str,
    signature: Option<&str>,
    file_content: &str,
    start_byte: usize,
    end_byte: usize,
) -> String {
    let mut card = String::with_capacity(512);
    card.push_str(rel_path.trim_start_matches("./"));
    card.push('\n');
    card.push_str(kind);
    card.push(' ');
    card.push_str(name);
    card.push('\n');
    if let Some(sig) = signature {
        if !sig.is_empty() && sig != name {
            card.push_str(sig);
            card.push('\n');
        }
    }
    if let Some(doc) = leading_doc_comment(file_content, start_byte) {
        card.push_str(&doc);
        card.push('\n');
    }

    let body = slice_bytes(file_content, start_byte, end_byte);
    card.push_str(truncate_at_char_boundary(body, BODY_BYTE_BUDGET));
    card
}

/// Hex SHA-256 of the exact card text. Stored per embedding row so a reindex
/// can tell "this symbol's embeddable content is unchanged" across runs —
/// symbol row ids are NOT stable across a full rebuild, the hash is.
pub fn card_hash(card: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(card.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Byte-offset slice that never panics on stale offsets: offsets are clamped
/// to the content and snapped outward/inward to char boundaries. Offsets come
/// from the index; the file may have changed since (staleness is handled
/// upstream, but a garbled card is still better than a crashed reindex).
fn slice_bytes(content: &str, start: usize, end: usize) -> &str {
    let len = content.len();
    let mut start = start.min(len);
    let mut end = end.min(len).max(start);
    while start < len && !content.is_char_boundary(start) {
        start += 1;
    }
    while end > start && !content.is_char_boundary(end) {
        end -= 1;
    }
    &content[start..end]
}

fn truncate_at_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut cut = max_bytes;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    &s[..cut]
}

/// The contiguous block of comment lines immediately above the symbol's
/// first line, if any: `///`, `//!`, `//`, `#`, `*`, `/*`, `/**`, `--`, or
/// JSDoc-style continuation lines. Language-agnostic on purpose — this runs
/// over every indexed language, and a false positive (an unrelated trailing
/// comment) only adds a line of mostly-relevant text to the card.
fn leading_doc_comment(content: &str, start_byte: usize) -> Option<String> {
    let head = slice_bytes(content, 0, start_byte);
    // The symbol may start mid-line (e.g. `export function foo`); drop the
    // partial line the symbol starts on before walking backwards.
    let head = match head.rfind('\n') {
        Some(pos) => &head[..pos],
        None => return None,
    };

    let mut doc_lines: Vec<&str> = Vec::new();
    for line in head.lines().rev() {
        let trimmed = line.trim();
        let is_comment = trimmed.starts_with("///")
            || trimmed.starts_with("//!")
            || trimmed.starts_with("//")
            || trimmed.starts_with('#')
            || trimmed.starts_with('*')
            || trimmed.starts_with("/*")
            || trimmed.starts_with("--");
        // Decorators/attributes sit between doc comment and symbol in Python/
        // Rust/TS; skip through them without ending the block.
        let is_decorator = trimmed.starts_with('@') || trimmed.starts_with("#[");
        if is_comment {
            doc_lines.push(trimmed);
        } else if is_decorator && doc_lines.is_empty() {
            continue;
        } else {
            break;
        }
        if doc_lines.len() >= 12 {
            break; // cap: a giant license header is not a doc comment
        }
    }
    if doc_lines.is_empty() {
        return None;
    }
    doc_lines.reverse();
    Some(doc_lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_contains_path_kind_name_signature_doc_and_body() {
        let source = "/// Formats how long ago a timestamp was.\nexport function timeAgo(d: Date): string {\n  return format(d);\n}\n";
        let start = source.find("export").unwrap();
        let card = build_card(
            "./lib/timeFormat.ts",
            "function",
            "timeAgo",
            Some("function timeAgo(d: Date): string"),
            source,
            start,
            source.len() - 1,
        );
        assert!(card.starts_with("lib/timeFormat.ts\n"));
        assert!(card.contains("function timeAgo"));
        assert!(card.contains("Formats how long ago"));
        assert!(card.contains("return format(d);"));
    }

    #[test]
    fn hash_changes_with_body_and_is_stable_otherwise() {
        let a = card_hash("lib/x.ts\nfunction f\nbody one");
        let b = card_hash("lib/x.ts\nfunction f\nbody one");
        let c = card_hash("lib/x.ts\nfunction f\nbody two");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn body_is_truncated_and_never_panics_on_bad_offsets() {
        let body = "x".repeat(10_000);
        let source = format!("function big() {{ {} }}", body);
        let card = build_card("./a.ts", "function", "big", None, &source, 0, source.len());
        assert!(card.len() < 2_500, "body must be truncated, got {}", card.len());

        // Stale offsets beyond EOF and inverted ranges degrade, not panic.
        let _ = build_card("./a.ts", "function", "big", None, "short", 9999, 4);
        // Multi-byte boundary safety.
        let uni = "fn é() { héllo }";
        let _ = build_card("./u.rs", "function", "é", None, uni, 4, uni.len());
    }
}
