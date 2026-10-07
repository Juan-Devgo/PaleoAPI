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
