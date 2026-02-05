use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("docs_generated.rs");
    let mut output = File::create(&dest_path).unwrap();

    let docs_dir = Path::new("src/docs");

    // Collect all .md files and their mappings
    let mut entries: Vec<(String, String)> = Vec::new();

    collect_docs(docs_dir, docs_dir, &mut entries);

    // Generate the Rust code
    writeln!(output, "fn get_docs() -> HashMap<&'static str, &'static str> {{").unwrap();
    writeln!(output, "    let mut docs = HashMap::new();").unwrap();

    for (key, file_path) in &entries {
        writeln!(
            output,
            "    docs.insert({:?}, include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), {:?})));",
            key,
            format!("/{}", file_path)
        )
        .unwrap();
    }

    writeln!(output, "    docs").unwrap();
    writeln!(output, "}}").unwrap();

    // Tell Cargo to rerun if docs change
    println!("cargo:rerun-if-changed=src/docs");
}

fn collect_docs(base_dir: &Path, current_dir: &Path, entries: &mut Vec<(String, String)>) {
    let Ok(read_dir) = fs::read_dir(current_dir) else {
        return;
    };

    for entry in read_dir.flatten() {
        let path = entry.path();

        if path.is_dir() {
            collect_docs(base_dir, &path, entries);
        } else if path.extension().is_some_and(|ext| ext == "md") {
            let rel_path = path.strip_prefix(base_dir).unwrap();
            let file_path = path.to_string_lossy().to_string();

            // Convert file path to documentation key
            let keys = path_to_keys(rel_path);
            for key in keys {
                entries.push((key, file_path.clone()));
            }
        }
    }
}

fn path_to_keys(rel_path: &Path) -> Vec<String> {
    let path_str = rel_path.to_string_lossy();
    let without_ext = path_str.trim_end_matches(".md");

    match without_ext {
        // index.md maps to root path ""
        "index" => vec!["".to_string()],
        // Everything else maps directly to its path
        _ => vec![without_ext.to_string()],
    }
}
