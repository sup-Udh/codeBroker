use crate::resolver::stages::ResolutionStage;
use crate::resolver::context::ResolutionContext;
use graph::models::ResolutionState;
use crate::resolver::decisions::{PipelineStageType, DecisionReason};

static PYTHON_STDLIB: &[&str] = &[
    "abc", "ast", "asynchat", "asyncio", "asyncore", "atexit", "base64", "bdb",
    "binascii", "bisect", "builtins", "bz2", "calendar", "cgi", "chunk", "cmath",
    "cmd", "code", "codecs", "codeop", "collections", "colorsys", "compileall",
    "concurrent", "configparser", "contextlib", "contextvars", "copy", "copyreg",
    "csv", "ctypes", "curses", "dataclasses", "datetime", "dbm", "decimal",
    "difflib", "dis", "distutils", "doctest", "email", "encodings", "enum", "errno",
    "filecmp", "fileinput", "fnmatch", "fractions", "ftplib", "functools", "gc",
    "getopt", "getpass", "gettext", "glob", "grp", "gzip", "hashlib", "heapq",
    "hmac", "html", "http", "imaplib", "importlib", "inspect", "io", "ipaddress",
    "itertools", "json", "keyword", "linecache", "locale", "logging", "lzma",
    "mailbox", "marshal", "math", "mimetypes", "mmap", "multiprocessing", "netrc",
    "numbers", "operator", "optparse", "os", "pathlib", "pdb", "pickle",
    "pickletools", "pkgutil", "platform", "pprint", "profile", "pstats", "pty",
    "pwd", "queue", "random", "re", "readline", "reprlib", "resource", "runpy",
    "sched", "secrets", "select", "selectors", "shelve", "shlex", "shutil",
    "signal", "site", "smtplib", "socket", "socketserver", "sqlite3", "ssl",
    "stat", "statistics", "string", "stringprep", "struct", "subprocess", "sys",
    "sysconfig", "tarfile", "tempfile", "test", "textwrap", "threading", "time",
    "timeit", "tkinter", "token", "tokenize", "tomllib", "traceback", "tracemalloc",
    "types", "typing", "unicodedata", "unittest", "urllib", "uuid", "venv",
    "warnings", "wave", "weakref", "webbrowser", "xml", "xmlrpc", "zipapp",
    "zipfile", "zipimport", "zlib", "zoneinfo", "_thread", "typing_extensions",
    "pydantic", // commonly treated as quasi-stdlib
];

pub(crate) static JS_BUILTIN_RECEIVERS: &[&str] = &[
    "console", "Math", "Object", "JSON", "Array", "String", "Number", "Boolean",
    "Symbol", "BigInt", "Promise", "Date", "RegExp", "Error", "Map", "Set",
    "WeakMap", "WeakSet", "WeakRef", "Proxy", "Reflect",
    "process", "window", "document", "globalThis", "self", "global",
    "performance", "crypto", "navigator", "location", "history",
    "localStorage", "sessionStorage", "sessionStorage", "indexedDB",
    "setTimeout", "clearTimeout", "setInterval", "clearInterval",
    "queueMicrotask", "requestAnimationFrame", "cancelAnimationFrame",
    "parseInt", "parseFloat", "isNaN", "isFinite",
    "encodeURIComponent", "decodeURIComponent", "encodeURI", "decodeURI",
    "fetch", "Response", "Request", "Headers", "URL", "URLSearchParams",
    "Blob", "File", "FileReader", "FormData",
    "XMLHttpRequest", "WebSocket", "EventSource",
    "Buffer", "TextEncoder", "TextDecoder",
    "AbortController", "AbortSignal",
    "MutationObserver", "IntersectionObserver", "ResizeObserver",
    "Event", "CustomEvent", "EventTarget",
    "setTimeout", "clearTimeout",
    "require", "module", "exports", "__dirname", "__filename",
];

static NODE_BUILTINS: &[&str] = &[
    "assert", "async_hooks", "buffer", "child_process", "cluster", "console",
    "constants", "crypto", "dgram", "diagnostics_channel", "dns", "domain",
    "events", "fs", "http", "http2", "https", "inspector", "module", "net", "os",
    "path", "perf_hooks", "process", "punycode", "querystring", "readline", "repl",
    "stream", "string_decoder", "sys", "timers", "tls", "trace_events", "tty",
    "url", "util", "v8", "vm", "worker_threads", "zlib",
];

pub struct ClassificationStage;

pub fn classify_import_source(
    source: &str,
    name: &str,
    is_rust: bool,
    is_js_ts: bool,
    is_python: bool,
    python_packages: &std::collections::HashSet<String>,
) -> ResolutionState {
    if source.starts_with("std::")
        || source.starts_with("core::")
        || source.starts_with("alloc::")
        || source == "std"
        || source == "core"
        || source == "alloc"
    {
        return ResolutionState::StandardLibrary;
    }

    if source.starts_with("crate::")
        || source.starts_with("super::")
        || source.starts_with("self::")
        || source == "crate"
        || source == "super"
        || source == "self"
    {
        return ResolutionState::RepositorySymbol;
    }

    if is_rust && !source.is_empty() {
        return ResolutionState::ExternalDependency;
    }

    // `@/...` and `~/...` are bundler/tsconfig path aliases (the default
    // convention in Next.js, Nuxt, and many Vite templates — tsconfig's
    // `"paths": {"@/*": ["./src/*"]}`), not npm scoped packages: a real
    // scoped package always has a non-empty scope between `@` and `/`
    // (`@babel/core`), so a bare `@/foo` is unambiguous. Without this check
    // every aliased import in an alias-based project was misclassified as
    // ExternalDependency, which halts MemberResolverStage immediately
    // instead of ever attempting to resolve the receiver's methods.
    let is_local_path_alias = source.starts_with("@/") || source.starts_with("~/");

    if is_js_ts && !source.is_empty()
        && !source.starts_with("./")
        && !source.starts_with("../")
        && !source.starts_with('/')
        && !is_local_path_alias
    {
        let base = source.split('/').next().unwrap_or(source);
        let base_trimmed = base.trim_start_matches('@');
        let is_node_builtin = NODE_BUILTINS.contains(&base)
            || source.starts_with("node:")
            || NODE_BUILTINS.contains(&base_trimmed);

        if is_node_builtin {
            return ResolutionState::Builtin;
        } else {
            return ResolutionState::ExternalDependency;
        }
    }

    if is_python {
        if source.starts_with('.') {
            return ResolutionState::RepositorySymbol;
        }

        let root = if !source.is_empty() {
            source.split('.').next().unwrap_or(source)
        } else {
            name.split('.').next().unwrap_or(name)
        };

        if PYTHON_STDLIB.contains(&root) {
            return ResolutionState::StandardLibrary;
        }

        // Python's absolute-import convention (`from services.Foo import Foo`,
        // the idiomatic default — PEP 8 discourages implicit relative imports)
        // is syntactically identical to importing a real third-party package.
        // Only a leading "." distinguishes an explicit relative import, which
        // is already handled above. Cross-reference against the project's own
        // package/module names instead of assuming "no leading dot" means
        // external — otherwise every first-party import in a typical Python
        // project is misclassified as ExternalDependency.
        if python_packages.contains(root) {
            return ResolutionState::RepositorySymbol;
        }

        return ResolutionState::ExternalDependency;
    }

    ResolutionState::RepositorySymbol
}

impl ResolutionStage for ClassificationStage {
    fn name(&self) -> &'static str {
        "ClassificationStage"
    }

    fn stage_type(&self) -> PipelineStageType {
        PipelineStageType::Classification
    }

    fn execute(&self, context: &mut ResolutionContext) -> Result<(), String> {
        let name = &context.ir.node.name;
        let kind = context.ir.node.kind.as_deref().unwrap_or("imports");

        if matches!(kind, "method_call" | "MEMBER_ACCESS") {
            let file_path = context.ctx.symbol_index.file_paths.get(&context.ir.source_file_id);
            let is_js_ts_for_recv = file_path.map(|p| {
                p.ends_with(".ts") || p.ends_with(".tsx")
                    || p.ends_with(".js") || p.ends_with(".jsx")
                    || p.ends_with(".mjs") || p.ends_with(".cjs")
                    || p.ends_with(".vue") || p.ends_with(".svelte")
            }).unwrap_or(false);

            if is_js_ts_for_recv {
                let receiver = context.ir.node.source.as_deref().unwrap_or("");
                if !receiver.starts_with("this.") && !receiver.starts_with("self.") && !receiver.is_empty() {
                    if JS_BUILTIN_RECEIVERS.contains(&receiver) {
                        context.resolve_with(
                            self.stage_type(),
                            ResolutionState::Builtin,
                            DecisionReason::BuiltinClassification,
                            None
                        );
                        return Ok(());
                    }
                }
            }
        }

        if !matches!(kind, "imports" | "re_export") {
            context.skip_stage(self.stage_type());
            return Ok(());
        }

        let source = context.ir.node.source.as_deref().unwrap_or("");
        let name = context.ir.node.name.as_str();

        if source.starts_with("std::")
            || source.starts_with("core::")
            || source.starts_with("alloc::")
            || source == "std"
            || source == "core"
            || source == "alloc"
        {
            context.resolve_with(
                self.stage_type(),
                ResolutionState::StandardLibrary,
                DecisionReason::StandardLibraryClassification,
                None
            );
            return Ok(());
        }

        if source.starts_with("crate::")
            || source.starts_with("super::")
            || source.starts_with("self::")
            || source == "crate"
            || source == "super"
            || source == "self"
        {
            context.emit_decision(self.stage_type(), crate::resolver::decisions::StageStatus::Success, Some(DecisionReason::RepositoryMatch), None, vec![]);
            return Ok(());
        }

        let file_path = context.ctx.symbol_index.file_paths.get(&context.ir.source_file_id);
        let is_rust = file_path.map(|p| p.ends_with(".rs")).unwrap_or(false);
        if is_rust && !source.is_empty() {
            context.resolve_with(
                self.stage_type(),
                ResolutionState::ExternalDependency,
                DecisionReason::ExternalDependencyClassification,
                None
            );
            return Ok(());
        }

        let is_js_ts = file_path.map(|p| {
            p.ends_with(".ts") || p.ends_with(".tsx")
                || p.ends_with(".js") || p.ends_with(".jsx")
                || p.ends_with(".mjs") || p.ends_with(".cjs")
                || p.ends_with(".vue") || p.ends_with(".svelte")
        }).unwrap_or(false);
        let is_python = file_path.map(|p| p.ends_with(".py")).unwrap_or(false);

        let state = classify_import_source(source, name, is_rust, is_js_ts, is_python, &context.ctx.symbol_index.python_packages);
        match state {
            ResolutionState::StandardLibrary => {
                context.resolve_with(
                    self.stage_type(),
                    ResolutionState::StandardLibrary,
                    DecisionReason::StandardLibraryClassification,
                    None
                );
            }
            ResolutionState::Builtin => {
                context.resolve_with(
                    self.stage_type(),
                    ResolutionState::Builtin,
                    DecisionReason::BuiltinClassification,
                    None
                );
            }
            ResolutionState::ExternalDependency => {
                context.resolve_with(
                    self.stage_type(),
                    ResolutionState::ExternalDependency,
                    DecisionReason::ExternalDependencyClassification,
                    None
                );
            }
            ResolutionState::RepositorySymbol => {
                context.emit_decision(self.stage_type(), crate::resolver::decisions::StageStatus::Success, Some(DecisionReason::RepositoryMatch), None, vec![]);
            }
            _ => {
                context.emit_decision(self.stage_type(), crate::resolver::decisions::StageStatus::Success, Some(DecisionReason::RepositoryMatch), None, vec![]);
            }
        }
        Ok(())
    }
}
