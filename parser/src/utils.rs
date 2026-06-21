pub fn is_noisy_call_name(name: &str) -> bool {
    matches!(
        name,
        "map" | "filter" | "reduce" | "forEach" | "push" | "pop" | "shift" | "unshift" |
        "splice" | "slice" | "join" | "split" | "replace" | "match" | "test" | "exec" |
        "log" | "info" | "warn" | "error" | "dir" | "keys" | "values" | "entries" | "assign" |
        "hasOwnProperty" | "toString" | "valueOf" | "setTimeout" | "setInterval" |
        "clearTimeout" | "clearInterval" | "require" | "stringify" | "parse" | "then" |
        "catch" | "finally" | "print" | "len" | "range" | "enumerate" | "zip" | "super" |
        "isinstance" | "issubclass" | "hasattr" | "getattr" | "setattr" | "delattr" | "type" |
        "id" | "hash" | "vars" | "locals" | "globals" | "open" | "read" | "write" |
        "close" | "append" | "extend" | "insert" | "remove" | "clear" | "index" | "count" |
        "sort" | "reverse" | "copy" | "update" | "setdefault" | "get" | "popitem" |
        "items" | "add" | "discard" | "difference" | "difference_update" |
        "intersection" | "intersection_update" | "isdisjoint" | "issubset" | "issuperset" |
        "symmetric_difference" | "symmetric_difference_update" | "union" | "find" | "find_symbol" | "read_file"
    )
}
