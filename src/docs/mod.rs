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

fn get_docs() -> HashMap<&'static str, &'static str> {
    let mut docs = HashMap::new();

    // Root documentation
    docs.insert("", include_str!("index.md"));

    // Workflow documentation
    docs.insert("workflow", include_str!("workflow.md"));

    // Migrations overview
    docs.insert("migrations", include_str!("migrations/actions.md"));
    docs.insert("migrations/actions", include_str!("migrations/actions.md"));

    // Individual action documentation
    docs.insert(
        "migrations/actions/create-table",
        include_str!("migrations/create-table.md"),
    );
    docs.insert(
        "migrations/actions/alter-column",
        include_str!("migrations/alter-column.md"),
    );
    docs.insert(
        "migrations/actions/add-column",
        include_str!("migrations/add-column.md"),
    );
    docs.insert(
        "migrations/actions/remove-column",
        include_str!("migrations/remove-column.md"),
    );
    docs.insert(
        "migrations/actions/rename-table",
        include_str!("migrations/rename-table.md"),
    );
    docs.insert(
        "migrations/actions/remove-table",
        include_str!("migrations/remove-table.md"),
    );
    docs.insert(
        "migrations/actions/add-index",
        include_str!("migrations/add-index.md"),
    );
    docs.insert(
        "migrations/actions/remove-index",
        include_str!("migrations/remove-index.md"),
    );
    docs.insert(
        "migrations/actions/create-enum",
        include_str!("migrations/create-enum.md"),
    );
    docs.insert(
        "migrations/actions/alter-enum",
        include_str!("migrations/alter-enum.md"),
    );
    docs.insert(
        "migrations/actions/remove-enum",
        include_str!("migrations/remove-enum.md"),
    );
    docs.insert(
        "migrations/actions/add-foreign-key",
        include_str!("migrations/add-foreign-key.md"),
    );
    docs.insert(
        "migrations/actions/remove-foreign-key",
        include_str!("migrations/remove-foreign-key.md"),
    );
    docs.insert(
        "migrations/actions/custom",
        include_str!("migrations/custom.md"),
    );

    docs
}
