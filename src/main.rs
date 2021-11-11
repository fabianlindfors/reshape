use std::{
    fs::{self, DirEntry, File},
    io::Read,
    path::{Path, PathBuf},
};

use clap::{Args, Parser};
use reshape::{
    migrations::{Action, Migration},
    Reshape,
};
use serde::{Deserialize, Serialize};

#[derive(Parser)]
struct Opts {
    #[clap(subcommand)]
    cmd: Command,
}

#[derive(Parser)]
enum Command {
    Migrate(MigrateOptions),
    Complete(ConnectionOptions),
    Remove(ConnectionOptions),
    Abort(ConnectionOptions),
    GenerateSchemaQuery,
}

#[derive(Args)]
struct MigrateOptions {
    #[clap(long, short)]
    complete: bool,
    #[clap(flatten)]
    connection_options: ConnectionOptions,
}

#[derive(Parser)]
struct ConnectionOptions {
    #[clap(long)]
    url: Option<String>,
    #[clap(long, default_value = "localhost")]
    host: String,
    #[clap(long, default_value = "5432")]
    port: u16,
    #[clap(long, short, default_value = "postgres")]
    database: String,
    #[clap(long, short, default_value = "postgres")]
    username: String,
    #[clap(long, short, default_value = "postgres")]
    password: String,
}

fn main() {
    let opts: Opts = Opts::parse();

    let result = run(opts);
    if let Err(e) = result {
        println!("Error: {}", e);
    }
}

fn run(opts: Opts) -> anyhow::Result<()> {
    match opts.cmd {
        Command::Migrate(opts) => {
            let mut reshape = reshape_from_connection_options(&opts.connection_options)?;
            let migrations = find_migrations()?;
            reshape.migrate(migrations)?;

            // Automatically complete migration if --complete flag is set
            if opts.complete {
                reshape.complete_migration()?;
            }

            Ok(())
        }
        Command::Complete(opts) => {
            let mut reshape = reshape_from_connection_options(&opts)?;
            reshape.complete_migration()
        }
        Command::Remove(opts) => {
            let mut reshape = reshape_from_connection_options(&opts)?;
            reshape.remove()
        }
        Command::Abort(opts) => {
            let mut reshape = reshape_from_connection_options(&opts)?;
            reshape.abort()
        }
        Command::GenerateSchemaQuery => {
            let migrations = find_migrations()?;
            let query = migrations
                .last()
                .map(|migration| reshape::schema_query_for_migration(&migration.name));
            println!("{}", query.unwrap_or_else(|| "".to_string()));

            Ok(())
        }
    }
}

fn reshape_from_connection_options(opts: &ConnectionOptions) -> anyhow::Result<Reshape> {
    let env_url = std::env::var("POSTGRES_URL").ok();
    let url = env_url.as_ref().or(opts.url.as_ref());

    match url {
        Some(url) => Reshape::new(&url),
        None => Reshape::new_with_options(&opts.host, opts.port, &opts.username, &opts.password),
    }
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
