//! `docs/openapi.yaml` describes exactly the routes the app serves
//! (AGENTS.md §5.4, research R12). Paths and methods are read by line pattern.

use std::collections::BTreeSet;

use paleo_api::api::ROUTES;

const METHODS: [&str; 7] = ["get", "post", "put", "patch", "delete", "head", "options"];

/// `(path, METHOD)` pairs under `paths:` of the published description.
fn documented() -> BTreeSet<(String, String)> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/docs/openapi.yaml");
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read docs/openapi.yaml: {e}"));
    let mut out = BTreeSet::new();
    let mut in_paths = false;
    let mut current: Option<String> = None;
    for line in text.lines() {
        if !line.starts_with(' ') && !line.trim().is_empty() && !line.starts_with('#') {
            in_paths = line.trim_end() == "paths:";
            current = None;
            continue;
        }
        if !in_paths {
            continue;
        }
        if let Some(rest) = line.strip_prefix("  /") {
            if let Some(p) = rest.trim_end().strip_suffix(':') {
                current = Some(format!("/{p}"));
            }
            continue;
        }
        if let (Some(p), Some(rest)) = (&current, line.strip_prefix("    ")) {
            if rest.starts_with(' ') {
                continue;
            }
            if let Some(m) = rest.trim_end().strip_suffix(':')
                && METHODS.contains(&m)
            {
                out.insert((p.clone(), m.to_ascii_uppercase()));
            }
        }
    }
    out
}

fn served() -> BTreeSet<(String, String)> {
    ROUTES
        .iter()
        .flat_map(|(path, methods)| {
            methods
                .iter()
                .map(move |m| (path.to_string(), m.as_str().to_string()))
        })
        .collect()
}

#[test]
fn openapi_matches_the_routes() {
    let documented = documented();
    let served = served();
    assert!(
        documented.len() > 50,
        "too few operations parsed: {documented:?}"
    );
    let undocumented: Vec<_> = served.difference(&documented).collect();
    let unserved: Vec<_> = documented.difference(&served).collect();
    assert!(
        undocumented.is_empty() && unserved.is_empty(),
        "served but not documented: {undocumented:?}\ndocumented but not served: {unserved:?}"
    );
}

/// Per-operation rules of 003 contracts/openapi.yaml, read by line pattern: the lines
/// between one operation key and the next are that operation's block.
fn operations() -> Vec<(String, String, String)> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/docs/openapi.yaml");
    let text = std::fs::read_to_string(path).expect("docs/openapi.yaml");
    let mut out: Vec<(String, String, String)> = Vec::new();
    let mut in_paths = false;
    let mut current = String::new();
    for line in text.lines() {
        if !line.starts_with(' ') && !line.trim().is_empty() && !line.starts_with('#') {
            in_paths = line.trim_end() == "paths:";
            continue;
        }
        if !in_paths {
            continue;
        }
        if let Some(rest) = line.strip_prefix("  /") {
            if let Some(p) = rest.trim_end().strip_suffix(':') {
                current = format!("/{p}");
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("    ")
            && !rest.starts_with(' ')
            && let Some(m) = rest.trim_end().strip_suffix(':')
            && METHODS.contains(&m)
        {
            out.push((current.clone(), m.to_string(), String::new()));
            continue;
        }
        if let Some(op) = out.last_mut() {
            op.2.push_str(line);
            op.2.push('\n');
        }
    }
    out
}

#[test]
fn operations_follow_the_security_rules() {
    let ops = operations();
    assert!(ops.len() > 50, "too few operations parsed: {}", ops.len());
    let mut problems = Vec::new();
    for (path, method, block) in &ops {
        let has = |code: &str| block.contains(&format!("'{code}':"));
        let mut need = vec!["429", "503"];
        if block.contains("requestBody:") {
            need.push("408");
        }
        let write = matches!(method.as_str(), "post" | "patch" | "delete");
        let login = path == "/auth/login" && method == "post";
        if write && !login {
            need.extend(["401", "403"]);
            if !block.contains("security: [{ adminBearer: [] }]") {
                problems.push(format!("{method} {path}: no adminBearer security"));
            }
        }
        for code in need {
            if !has(code) {
                problems.push(format!("{method} {path}: no '{code}' response"));
            }
        }
        let open = block.contains("security: []");
        if open != login {
            problems.push(format!("{method} {path}: `security: []` only on login"));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
