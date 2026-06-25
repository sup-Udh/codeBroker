//! Shared, single-source-of-truth classification of "entrypoints" (the symbols
//! a repository exposes to the outside world: HTTP/WebSocket route handlers and
//! framework page/layout components).
//!
//! Every tool that reports entrypoints (`list_entrypoints`, `project_overview`,
//! `repository_stats`, `subsystem_stats`, and the `is_entrypoint` feature flag
//! computed at index time) routes through these functions, so they can never
//! disagree about what counts as a route vs. a page vs. nothing. Previously the
//! logic was duplicated and inconsistent: feature extraction used a naive
//! `contains("@")` decorator check (which both missed `@app.websocket` framing
//! and falsely flagged `@staticmethod`/`@property`), and the route-vs-page split
//! was re-derived from the symbol `kind` alone in three different places — none
//! of which recognised Next.js App Router (`app/**/page.tsx`) file conventions,
//! so every such entrypoint silently reported as zero.

/// How an entrypoint is surfaced to callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntrypointClass {
    /// An API/WebSocket route handler (FastAPI/Flask decorator, Next.js
    /// `route.ts` / `pages/api`, an explicitly-parsed `route`/`endpoint` kind).
    Route,
    /// A user-facing page/layout component (Next.js App Router `page`/`layout`,
    /// Pages Router page, or an explicitly-parsed `page`/`layout` kind).
    Page,
    /// A CLI/desktop application entrypoint: Python `__main__.py` convention or
    /// a module-level function named `main` in a Python script.
    Cli,
}

impl EntrypointClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            EntrypointClass::Route => "route",
            EntrypointClass::Page => "page",
            EntrypointClass::Cli => "cli",
        }
    }
}

/// Symbol kinds that can act as a callable entrypoint target. Mirrors the
/// callable set used by feature extraction, plus `jsx_element` (the synthetic
/// symbol the TS frontend emits for a rendered component body).
fn is_callable_kind(kind_lower: &str) -> bool {
    matches!(
        kind_lower,
        "function" | "method" | "route" | "endpoint" | "lambda" | "jsx_element" | "component"
    )
}

/// True if a decorator string denotes an HTTP/WebSocket route. Handles the
/// common Python frameworks: FastAPI/Starlette (`@app.get(...)`,
/// `@router.post(...)`, `@app.websocket(...)`), Flask/Blueprint
/// (`@app.route(...)`, `@bp.route(...)`), and bare `@get`/`@post` style
/// decorators. Crucially it does NOT fire for ordinary decorators like
/// `@staticmethod`, `@property`, `@dataclass`, or `@pytest.fixture`, which the
/// old `contains("@")` heuristic wrongly promoted to entrypoints.
pub fn is_route_decorator(attr: &str) -> bool {
    // Strip a leading '@' and take everything up to the first '(' — the
    // "callee" portion, e.g. "app.get" from `@app.get("/x")`.
    let head = attr.trim().trim_start_matches('@');
    let head = head.split('(').next().unwrap_or("").trim();
    if head.is_empty() {
        return false;
    }

    const METHODS: &[&str] = &[
        "get",
        "post",
        "put",
        "delete",
        "patch",
        "head",
        "options",
        "websocket",
        "route",
        "api_route",
        "websocket_route",
    ];

    // Dotted form: `app.get`, `router.post`, `bp.route`, `app.websocket`.
    if let Some((_obj, method)) = head.rsplit_once('.') {
        return METHODS.contains(&method.to_lowercase().as_str());
    }

    // Bare form: `@get`, `@post` (some micro-frameworks / decorators-as-router).
    METHODS.contains(&head.to_lowercase().as_str())
}

/// Lowercased file extension of `path`, or "" if none.
fn extension(path: &str) -> String {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
}

/// File stem (name without extension), lowercased.
fn stem(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase()
}

fn is_web_component_ext(ext: &str) -> bool {
    matches!(ext, "tsx" | "jsx" | "ts" | "js")
}

/// Detects Next.js (and similar) file-convention entrypoints purely from the
/// path. App Router: `app/**/page.{tsx,jsx,ts,js}` and `layout.*` are pages,
/// `app/**/route.{ts,js}` is an API route. Pages Router: `pages/**/*.{tsx,jsx}`
/// is a page, `pages/api/**` is a route. `name`/`kind` gate which symbol inside
/// such a file is treated as the entrypoint so helper functions don't all get
/// flagged.
fn nextjs_class(path: &str, name: &str, kind_lower: &str) -> Option<EntrypointClass> {
    let normalized = path.replace('\\', "/");
    let lower = normalized.to_lowercase();
    let ext = extension(&normalized);
    if !is_web_component_ext(&ext) {
        return None;
    }
    let file_stem = stem(&normalized);

    let in_app_dir = lower.contains("/app/") || lower.starts_with("app/");
    let in_pages_dir = lower.contains("/pages/") || lower.starts_with("pages/");

    // App Router API route (`app/**/route.ts`): handler functions are named
    // after the HTTP verb (GET/POST/...), so accept any callable.
    if in_app_dir && file_stem == "route" && (ext == "ts" || ext == "js") {
        if is_callable_kind(kind_lower) {
            return Some(EntrypointClass::Route);
        }
        return None;
    }

    // App Router page/layout: the default-exported component. Restrict to
    // callable symbols whose name is component-cased (leading uppercase),
    // which is the React component convention (e.g. `Home`, `RootLayout`),
    // so co-located lowercase helpers aren't mistaken for the page.
    if in_app_dir
        && matches!(
            file_stem.as_str(),
            "page" | "layout" | "template" | "default"
        )
    {
        if is_callable_kind(kind_lower)
            && name
                .chars()
                .next()
                .map(|c| c.is_uppercase())
                .unwrap_or(false)
        {
            return Some(EntrypointClass::Page);
        }
        return None;
    }

    // Pages Router.
    if in_pages_dir && (ext == "tsx" || ext == "jsx") {
        // `_app`/`_document` are framework plumbing, not user pages.
        if file_stem == "_app" || file_stem == "_document" {
            return None;
        }
        if !is_callable_kind(kind_lower) {
            return None;
        }
        let is_component = name
            .chars()
            .next()
            .map(|c| c.is_uppercase())
            .unwrap_or(false);
        if !is_component {
            return None;
        }
        if lower.contains("/pages/api/") {
            return Some(EntrypointClass::Route);
        }
        return Some(EntrypointClass::Page);
    }

    None
}

/// The single classifier every entrypoint-aware code path consults. Returns
/// `Some(Route|Page)` if `(name, kind, path, attributes)` describe an
/// entrypoint, else `None`. `attributes` is the symbol's decorator list (as the
/// parser captured it, e.g. `["@app.get(\"/\")"]`).
pub fn classify_entrypoint(
    name: &str,
    kind: &str,
    path: &str,
    attributes: &[String],
) -> Option<EntrypointClass> {
    let kind_lower = kind.to_lowercase();

    // 1. Explicit parser-assigned kinds win outright.
    match kind_lower.as_str() {
        "page" | "layout" => return Some(EntrypointClass::Page),
        "route" | "endpoint" => return Some(EntrypointClass::Route),
        _ => {}
    }

    // 2. Decorator-based routes (FastAPI/Flask/Starlette).
    if is_callable_kind(&kind_lower) {
        for attr in attributes {
            if is_route_decorator(attr) {
                return Some(EntrypointClass::Route);
            }
        }
    }

    // 3. Framework file conventions (Next.js App Router / Pages Router).
    if let Some(c) = nextjs_class(path, name, &kind_lower) {
        return Some(c);
    }

    // 4. CLI/desktop entrypoints (Python only).
    //    `__main__.py` is the Python package entry-point convention: any
    //    top-level callable or class in that file is an application entrypoint.
    if path.ends_with("__main__.py") && (is_callable_kind(&kind_lower) || kind_lower == "class") {
        return Some(EntrypointClass::Cli);
    }
    //    `def main(...)` at module level in any .py file is the conventional
    //    CLI entry function (typically called under `if __name__ == "__main__"`).
    if kind_lower == "function" && name == "main" && path.ends_with(".py") {
        return Some(EntrypointClass::Cli);
    }

    None
}

/// Convenience wrapper that parses a JSON-encoded attribute list (the form
/// stored in `symbols.attributes`) before classifying. A `None`/invalid
/// attributes column is treated as an empty decorator list.
pub fn classify_entrypoint_json(
    name: &str,
    kind: &str,
    path: &str,
    attributes_json: Option<&str>,
) -> Option<EntrypointClass> {
    let attrs: Vec<String> = attributes_json
        .and_then(|j| serde_json::from_str(j).ok())
        .unwrap_or_default();
    classify_entrypoint(name, kind, path, &attrs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fastapi_decorators_are_routes() {
        for d in [
            "@app.get(\"/\")",
            "@app.post(\"/analyze-topology\")",
            "@app.websocket(\"/simulation/{id}/metrics\")",
            "@router.post(\"/import-topology\")",
            "@app.route(\"/x\", methods=[\"POST\"])",
            "@bp.route(\"/y\")",
        ] {
            assert!(is_route_decorator(d), "{d} should be a route decorator");
        }
    }

    #[test]
    fn ordinary_decorators_are_not_routes() {
        for d in [
            "@staticmethod",
            "@property",
            "@dataclass",
            "@pytest.fixture",
            "@classmethod",
            "@functools.lru_cache()",
        ] {
            assert!(!is_route_decorator(d), "{d} must NOT be a route decorator");
        }
    }

    #[test]
    fn decorated_function_classifies_as_route() {
        let attrs = vec!["@app.websocket(\"/simulation/{id}/metrics\")".to_string()];
        assert_eq!(
            classify_entrypoint(
                "simulation_metrics",
                "function",
                "OrcaAI/orchestrator/main.py",
                &attrs
            ),
            Some(EntrypointClass::Route)
        );
    }

    #[test]
    fn nextjs_app_router_page_and_layout() {
        assert_eq!(
            classify_entrypoint("Home", "function", "OrcaAI/frontend/app/page.tsx", &[]),
            Some(EntrypointClass::Page)
        );
        assert_eq!(
            classify_entrypoint(
                "RootLayout",
                "function",
                "OrcaAI/frontend/app/layout.tsx",
                &[]
            ),
            Some(EntrypointClass::Page)
        );
    }

    #[test]
    fn nextjs_lowercase_helper_in_page_is_not_entrypoint() {
        assert_eq!(
            classify_entrypoint(
                "formatDate",
                "function",
                "OrcaAI/frontend/app/page.tsx",
                &[]
            ),
            None
        );
    }

    #[test]
    fn nextjs_app_router_route_is_a_route() {
        assert_eq!(
            classify_entrypoint("GET", "function", "app/api/users/route.ts", &[]),
            Some(EntrypointClass::Route)
        );
    }

    #[test]
    fn plain_function_is_not_an_entrypoint() {
        assert_eq!(
            classify_entrypoint("helper", "function", "src/util.py", &[]),
            None
        );
    }
}
