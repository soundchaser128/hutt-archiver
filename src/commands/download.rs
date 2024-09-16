use std::collections::HashMap;
use std::process::Stdio;
use std::time::Duration;

use camino::{Utf8Path, Utf8PathBuf};
use color_eyre::eyre::bail;
use indicatif::{ProgressBar, ProgressStyle};
use tokio::io::AsyncWriteExt;
use tracing::{debug, info};

use crate::commands::metadata::USER_AGENT;
use crate::database::{LinkStatus, PostLink, PostType, StatusUpdate};
use crate::filenames::get_download_path;
use crate::{DownloadContext, Result};

const BASE_URL: &str = "https://hutt.co";

#[derive(Debug)]
pub struct DownloadArgs {
    pub filename_pattern: HashMap<PostType, String>,
    pub path: Utf8PathBuf,
    pub dry_run: bool,
    pub progress: bool,
    pub fail_fast: bool,
}

async fn download_video(
    context: &DownloadContext,
    link: &PostLink,
    file: impl AsRef<Utf8Path>,
) -> Result<()> {
    use tokio::process::Command;

    let directory = file.as_ref().parent().unwrap();
    tokio::fs::create_dir_all(directory).await?;

    let file_name = file.as_ref().file_name().unwrap();

    let referer = format!("https://hutt.co/{}", context.configuration.creator_name);

    let url = format!("{}{}", BASE_URL, link.url);
    info!("video link: {}", url);
    let mut command = Command::new("yt-dlp")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .arg("--add-header")
        .arg(format!("Cookie: {}", context.configuration.cookie))
        .arg("--add-header")
        .arg(format!("User-Agent: {}", USER_AGENT))
        .arg("--add-header")
        .arg(format!("Referer: {}", referer))
        .arg("-N")
        .arg("3")
        .arg("-R")
        .arg("3")
        .arg("--retry-sleep")
        .arg("120")
        .arg("-o")
        .arg(file_name)
        .arg(&url)
        .current_dir(directory)
        .spawn()?;

    let result = command.wait().await?;
    if !result.success() {
        bail!("failed to download {} with exit code {}", link.url, result);
    } else {
        info!("downloaded {} to {}", url, directory);
    }

    Ok(())
}

async fn download_images(
    context: &DownloadContext,
    link: &PostLink,
    file: impl AsRef<Utf8Path>,
) -> Result<()> {
    use tokio::fs::File;

    let directory = file.as_ref().parent().unwrap();
    tokio::fs::create_dir_all(directory).await?;

    let url = format!("{}{}", BASE_URL, link.url);
    let mut response = context
        .client
        .get(&url)
        .header("Cookie", &context.configuration.cookie)
        .header("User-Agent", USER_AGENT)
        .send()
        .await?
        .error_for_status()?;
    info!(
        "downloaded {} with status {} to {}",
        url,
        response.status(),
        file.as_ref()
    );
    let mut file = File::create(file.as_ref()).await?;
    while let Some(chunk) = response.chunk().await? {
        file.write_all(&chunk).await?;
    }

    Ok(())
}

pub async fn run(context: DownloadContext, args: DownloadArgs) -> Result<()> {
    let posts = context.database.fetch_all().await?;
    let posts: Vec<_> = posts
        .into_iter()
        .filter(|post| {
            post.links
                .iter()
                .any(|link| link.status != LinkStatus::Downloaded)
        })
        .collect();

    let db = &context.database;
    let progress = if args.progress {
        ProgressBar::new(posts.iter().map(|post| post.links.len()).sum::<usize>() as u64)
    } else {
        ProgressBar::hidden()
    };

    let style = ProgressStyle::with_template(
        "[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}",
    )
    .unwrap();
    progress.set_style(style);

    for post in posts.iter() {
        info!("post {}: type {:?}", post.id, post.post_type);

        for link in &post.links {
            let pattern = &args.filename_pattern[&post.post_type];
            let filename = get_download_path(post, link.id, pattern, &args.path);
            progress.set_message(format!("Downloading {filename}"));
            info!("Downloading link {}/{} to {}", post.id, link.id, filename);
            if filename.is_file() {
                info!(
                    "File {} already exists, skipping and updating state in database",
                    filename
                );
                db.update_status(
                    link.id,
                    StatusUpdate::Success {
                        file_path: filename.to_string(),
                        file_path_pattern: pattern.to_string(),
                    },
                )
                .await?;
                progress.inc(1);
                continue;
            }
            if !args.dry_run {
                let result = match post.post_type {
                    PostType::Video => download_video(&context, &link, &filename).await,
                    PostType::Image => download_images(&context, &link, &filename).await,
                };

                match result {
                    Ok(_) => {
                        db.update_status(
                            link.id,
                            StatusUpdate::Success {
                                file_path: filename.to_string(),
                                file_path_pattern: pattern.to_string(),
                            },
                        )
                        .await?
                    }
                    Err(e) => {
                        db.update_status(
                            link.id,
                            StatusUpdate::Error {
                                error: e.to_string(),
                            },
                        )
                        .await?;

                        if args.fail_fast {
                            return Err(e);
                        }
                    }
                }
            } else {
                tokio::time::sleep(Duration::from_millis(100)).await;
                debug!("Dry run: not updating status for post {}", post.id);
            }
            progress.inc(1);
        }
    }

    Ok(())
}
