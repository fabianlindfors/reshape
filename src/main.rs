use std::{
    fs::{self, DirEntry, File},
    io::Read,
    path::{Path, PathBuf},
};

use clap::Parser;
use reshape::{
    migrations::{Action, Migration},
    Reshape,
};
use serde::{Deserialize, Serialize};

#[derive(Parser)]
struct Opts {
    #[clap(subcommand)]
    cmd: Command,
    #[clap(default_value = "postgres://postgres:postgres@localhost:5432/postgres")]
    url: String,
}

#[derive(Parser)]
enum Command {
    Migrate,
    Finish,
    Remove,
    LatestSchema,
    Abort,
}

fn main() {
    let opts: Opts = Opts::parse();

    let result = run(opts);
    if let Err(e) = result {
        println!("Error: {}", e);
    }
}

fn run(opts: Opts) -> anyhow::Result<()> {
    let mut reshape = Reshape::new(&opts.url)?;

    match opts.cmd {
        Command::Migrate => migrate(&mut reshape),
        Command::Finish => reshape.complete_migration(),
        Command::Remove => reshape.remove(),
        Command::LatestSchema => {
            println!(
                "{}",
                reshape.latest_schema().unwrap_or_else(|| "".to_string())
            );
            Ok(())
        }
        Command::Abort => reshape.abort(),
    }
}

fn migrate(reshape: &mut Reshape) -> anyhow::Result<()> {
    let migrations = find_migrations()?;
    reshape.migrate(migrations)
}

fn find_migrations() -> anyhow::Result<Vec<Migration>> {
    let path = Path::new("migrations");

    // Return early if path doesn't exist
    if !path.exists() {
        return Ok(Vec::new());
    }

    let mut paths: Vec<PathBuf> = fs::read_dir(path)?
        .collect::<std::io::Result<Vec<DirEntry>>>()?
        .iter()
        .map(|entry| entry.path())
        .collect();

    // Sort all files by their file names (without extension)
    paths.sort_unstable_by_key(|path| path.as_path().file_stem().unwrap().to_os_string());

    paths
        .iter()
        .map(|path| {
            let mut file = File::open(path)?;

            // Read file data
            let mut data = String::new();
            file.read_to_string(&mut data)?;

            Ok((path, data))
        })
        .map(|result| {
            result.and_then(|(path, data)| {
                let extension = path.extension().and_then(|ext| ext.to_str()).unwrap();
                let file_migration = decode_migration_file(&data, extension)?;

                let file_name = path.file_stem().and_then(|name| name.to_str()).unwrap();
                Ok(Migration {
                    name: file_migration.name.unwrap_or_else(|| file_name.to_string()),
                    description: file_migration.description,
                    actions: file_migration.actions,
                })
            })
        })
        .collect()
}

fn decode_migration_file(data: &str, extension: &str) -> anyhow::Result<FileMigration> {
    let migration: FileMigration = match extension {
        "json" => serde_json::from_str(data)?,
        "toml" => toml::from_str(data)?,
        extension => {
            return Err(anyhow::anyhow!(
                "unrecognized file extension '{}'",
                extension
            ))
        }
    };

    Ok(migration)
}

#[derive(Serialize, Deserialize)]
struct FileMigration {
    name: Option<String>,
    description: Option<String>,
    actions: Vec<Box<dyn Action>>,
}
