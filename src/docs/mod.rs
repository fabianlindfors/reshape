use std::collections::HashMap;

use anyhow::anyhow;

/// Get documentation content for a given path.
///
/// Paths are normalized by stripping leading `/` characters.
/// Returns an error if the path is not found, listing available paths.
pub fn get(path: &str) -> anyhow::Result<&'static str> {
    let docs = get_docs();

    // Normalize path by stripping leading slashes
    let normalized_path = path.trim_start_matches('/');

    docs.get(normalized_path).copied().ok_or_else(|| {
        let mut available: Vec<_> = docs.keys().copied().collect();
        available.sort();

        anyhow!(
            "Documentation path '{}' not found.\n\nAvailable paths:\n{}",
            path,
            available
                .iter()
                .map(|p| format!("  /{}", p))
                .collect::<Vec<_>>()
                .join("\n")
        )
    })
}

include!(concat!(env!("OUT_DIR"), "/docs_generated.rs"));
