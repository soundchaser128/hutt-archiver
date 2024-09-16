use camino::Utf8Path;
use tracing::{debug, info, warn};

use crate::database::LinkStatus;
use crate::{filenames, DownloadContext, Result};

async fn do_rename(
    link_id: i64,
    current_path: &Utf8Path,
    new_path: &Utf8Path,
    pattern: &str,
    context: &DownloadContext,
) -> Result<()> {
    let parent = new_path.parent().expect("must have parent");

    tokio::fs::create_dir_all(parent).await?;
    tokio::fs::rename(&current_path, &new_path).await?;
    let db_result = context
        .database
        .update_path(link_id, new_path.as_str(), pattern)
        .await;
    if let Err(e) = db_result {
        warn!(
            "failed to update database for link ID {}, rolling back rename",
            link_id
        );
        tokio::fs::rename(&new_path, &current_path).await?;
        return Err(e);
    }

    Ok(())
}

fn remove_empty_directories(base_path: &Utf8Path) -> Result<()> {
    use walkdir::WalkDir;

    for entry in WalkDir::new(&base_path) {
        let entry = entry?;
        if entry.path().is_dir() {
            let is_empty = entry.path().read_dir()?.next().is_none();
            if is_empty {
                info!("removing empty directory '{}'", entry.path().display());
                std::fs::remove_dir(entry.path())?;
            }
        }
    }

    Ok(())
}

pub async fn run(dry_run: bool, context: DownloadContext) -> Result<()> {
    let posts = context.database.fetch_all().await?;
    let filename_patterns = context.configuration.filename_pattern();

    for post in &posts {
        for link in &post.links {
            if link.status == LinkStatus::Downloaded {
                let current_path = link
                    .file_path
                    .as_deref()
                    .expect("must be set for downloaded files");
                let current_path = Utf8Path::new(current_path);

                let pattern = &filename_patterns[&post.post_type];
                let new_path = filenames::get_download_path(
                    &post,
                    link.id,
                    pattern,
                    context.configuration.download_directory(),
                );

                if current_path != new_path {
                    if !Utf8Path::new(current_path).is_file() {
                        warn!("{} does not exist, skipping", current_path);
                        continue;
                    }
                    info!("'{}' -> '{}'", current_path, new_path);
                    if !dry_run {
                        do_rename(link.id, current_path, &new_path, &pattern, &context).await?;
                    }
                } else {
                    debug!("skipping {} as it is already renamed", current_path);
                }
            }
        }
    }

    if !dry_run {
        remove_empty_directories(context.configuration.download_directory())?;
    }
    Ok(())
}
