use std::collections::HashMap;

use camino::{Utf8Path, Utf8PathBuf};
use clap::{Parser, Subcommand};
use reqwest::Client;
use serde::Deserialize;
use sqlx::SqlitePool;
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::commands::download::DownloadArgs;
use crate::commands::metadata::MetadataArgs;
use crate::commands::set_dates::SetDatesArgs;
use crate::database::{Database, LinkStatus, PostType};

mod commands;
mod database;
mod filenames;

pub type Result<T> = color_eyre::Result<T>;

pub struct DownloadContext {
    pub database: Database,
    pub client: Client,
    pub configuration: Configuration,
}

impl DownloadContext {
    pub fn new(pool: SqlitePool, configuration: Configuration) -> Self {
        Self {
            database: Database::new(pool),
            client: Client::new(),
            configuration,
        }
    }
}

#[derive(Parser, Debug)]
pub struct Args {
    #[clap(short, long)]
    pub log: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Gathers all the metadata for the creator in the database.
    Metadata,

    /// Downloads all the not-yet downloaded media for the creator that's stored in the database.
    Download {
        #[clap(short, long)]
        dry_run: bool,
    },

    /// Reset the status of all downloads to `Pending`.
    ResetDownloads,

    /// Creates a backup of the database.
    BackupDatabase,

    /// Prints a report of the current state of the database.
    Report,

    /// Renames all the files in the database to match the new filename pattern.
    Rename {
        #[clap(short, long)]
        dry_run: bool,
    },

    /// Sets the dates for all posts in the database to a range between `start` and `end`. It will interpolate the dates between the two.
    /// This means, the first post will have the date of `start` and the last post will have the date of `end`, with all the posts in between having dates in between.
    SetDates { start: String, end: String },
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Configuration {
    pub cookie: String,
    pub creator_id: i64,
    pub creator_name: String,
    pub filename_pattern: Option<HashMap<PostType, String>>,

    pub download_directory: Option<Utf8PathBuf>,
}

impl Configuration {
    pub fn load() -> Result<Self> {
        const DEFAULT_CONFIG: &'static str = include_str!("../config.example.json5");

        let path = Utf8Path::new("config.json5");
        let config = if path.is_file() {
            let content = std::fs::read_to_string(path)?;
            json5::from_str(&content)?
        } else {
            println!("Created default configuration file at `config.json5`.");
            println!("Short instructions:");
            println!("");

            println!("1. Log in to Hutt in your browser.");
            println!("2. Open the developer tools (F12) and go to the Network tab.");
            println!("3. Refresh the page.");
            println!(
                "4. Find the request to any of the API endpoints and copy the `Cookie` header."
            );
            println!(
                "5. Paste the `Cookie` header into the `cookie` field in the configuration file."
            );
            println!("6. Find the numerical ID of the creator by looking at the `/is-live?id=...` request. The number at the end is their ID.");
            println!("7. Set the `creatorId` and `creatorName` fields to the creator you want to download.");

            std::fs::write(path, DEFAULT_CONFIG)?;
            std::process::exit(1);
        };

        Ok(config)
    }

    pub fn download_directory(&self) -> &Utf8Path {
        self.download_directory
            .as_deref()
            .unwrap_or_else(|| Utf8Path::new("downloads"))
    }

    pub fn filename_pattern(&self) -> HashMap<PostType, String> {
        self.filename_pattern.clone().unwrap_or_else(|| {
            [
                (
                    PostType::Image,
                    "{type}/{post_id} - {title}/{link_id}".to_string(),
                ),
                (PostType::Video, "{type}/{post_id} - {title}".to_string()),
            ]
            .iter()
            .cloned()
            .collect()
        })
    }

    #[cfg(test)]
    pub fn test() -> Self {
        Self {
            download_directory: Some(Utf8PathBuf::from("downloads")),
            cookie: "cookie".to_string(),
            creator_id: 1,
            creator_name: "creator".to_string(),
            filename_pattern: Some(
                [
                    (PostType::Image, "{link_id}".to_string()),
                    (PostType::Video, "{link_id}".to_string()),
                ]
                .iter()
                .cloned()
                .collect(),
            ),
        }
    }
}

async fn print_report(context: DownloadContext) -> Result<()> {
    let posts = context.database.fetch_all().await?;
    let total_count: usize = posts.iter().map(|p| p.links.len()).sum();
    let downloaded_count: usize = posts
        .iter()
        .map(|p| {
            p.links
                .iter()
                .filter(|l| l.status == LinkStatus::Downloaded)
                .count()
        })
        .sum();

    let error_count: usize = posts
        .iter()
        .map(|p| {
            p.links
                .iter()
                .filter(|l| l.status == LinkStatus::Error)
                .count()
        })
        .sum();

    let pending_count: usize = posts
        .iter()
        .map(|p| {
            p.links
                .iter()
                .filter(|l| l.status == LinkStatus::Pending)
                .count()
        })
        .sum();

    println!("Total links: {}", total_count);
    println!("Downloaded links: {}", downloaded_count);
    println!("Error links: {}", error_count);
    println!("Pending links: {}", pending_count);

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();

    if args.log {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::new("info"))
            .init();
    }

    let config = Configuration::load()?;
    let pool = SqlitePool::connect("sqlite:hutt.sqlite3").await?;
    let context = DownloadContext {
        database: Database::new(pool),
        client: Client::new(),
        configuration: config.clone(),
    };

    info!("Running with args: {:?}", args);

    match args.command {
        Command::Metadata {} => {
            commands::metadata::run(
                context,
                MetadataArgs {
                    creator_id: config.creator_id,
                    creator_name: config.creator_name,
                    cookie: config.cookie,
                },
            )
            .await?;
        }
        Command::Download { dry_run } => {
            commands::download::run(
                context,
                DownloadArgs {
                    filename_pattern: config.filename_pattern(),
                    path: config.download_directory().to_owned(),
                    dry_run,
                    progress: !args.log,
                    fail_fast: true,
                },
            )
            .await?
        }
        Command::ResetDownloads => {
            context.database.reset_downloads().await?;
        }
        Command::BackupDatabase => {
            let backup_path = format!(
                "hutt.{}.sqlite3",
                chrono::Utc::now().format("%Y-%m-%d_%H-%M-%S")
            );
            std::fs::copy("hutt.sqlite3", backup_path)?;
        }
        Command::Report => print_report(context).await?,
        Command::Rename { dry_run } => {
            commands::rename::run(dry_run, context).await?;
        }
        Command::SetDates { start, end } => {
            commands::set_dates::run(context, SetDatesArgs { start, end }).await?;
        }
    }
    Ok(())
}
