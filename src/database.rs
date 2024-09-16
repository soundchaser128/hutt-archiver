use std::collections::BTreeMap;

use chrono::NaiveDate;
use color_eyre::Result;
use serde::{Deserialize, Serialize};
use sqlx::prelude::Type;
use sqlx::SqlitePool;
use tracing::info;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum LinkSource {
    ImageGallery,
    VideoPost,
    HtmlString,
}

impl From<String> for LinkSource {
    fn from(s: String) -> Self {
        match s.as_str() {
            "image-gallery" | "ImageGallery" => LinkSource::ImageGallery,
            "video-post" | "VideoPost" => LinkSource::VideoPost,
            "html-string" | "HtmlString" => LinkSource::HtmlString,
            _ => panic!("Invalid link source: {}", s),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PostLink {
    pub id: i64,
    pub url: String,
    pub content_type: String,
    pub source: LinkSource,
    pub status: LinkStatus,
    pub error: Option<String>,
    pub file_path: Option<String>,
    pub file_path_pattern: Option<String>,
}

#[derive(Debug)]
pub struct CreatePostLink {
    pub url: String,
    pub content_type: String,
    pub source: LinkSource,
}

#[derive(Debug, Type, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LinkStatus {
    Pending,
    Downloaded,
    Error,
}

impl From<String> for LinkStatus {
    fn from(s: String) -> Self {
        match s.as_str() {
            "pending" | "Pending" => LinkStatus::Pending,
            "downloaded" | "Downloaded" => LinkStatus::Downloaded,
            "error" | "Error" => LinkStatus::Error,
            _ => panic!("Invalid link status: {}", s),
        }
    }
}

#[derive(Debug, Type, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum PostType {
    Video,
    Image,
}

impl From<String> for PostType {
    fn from(s: String) -> Self {
        match s.as_str() {
            "Video" | "video" => PostType::Video,
            "Image" | "image" => PostType::Image,
            _ => panic!("Invalid post type: {}", s),
        }
    }
}

#[derive(Debug)]
pub struct CreatePost {
    pub id: i64,
    pub title: String,
    pub creator: String,
    pub tags: Vec<String>,
    pub post_type: PostType,
    pub like_count: i64,
    pub links: Vec<CreatePostLink>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Post {
    pub id: i64,
    pub title: String,
    pub creator: String,
    pub tags: Vec<String>,
    pub post_type: PostType,
    pub like_count: i64,
    pub links: Vec<PostLink>,
    pub generated_title: Option<String>,
    pub created_at: Option<NaiveDate>,
}

#[derive(Debug)]
pub enum StatusUpdate {
    Success {
        file_path: String,
        file_path_pattern: String,
    },
    Error {
        error: String,
    },
    Pending,
}

struct JoinedPost {
    // Post fields
    pub id: i64,
    pub title: String,
    pub creator: String,
    pub tags: String,
    pub post_type: PostType,
    pub like_count: i64,
    pub generated_title: Option<String>,
    pub created_at: Option<String>,

    // PostLink fields
    pub rowid: i64,
    pub url: String,
    pub content_type: String,
    pub source: LinkSource,
    pub status: LinkStatus,
    pub error: Option<String>,
    pub file_path: Option<String>,
    pub file_path_pattern: Option<String>,
}

fn to_hutt_post(posts: Vec<JoinedPost>) -> Post {
    let first = &posts[0];
    Post {
        id: first.id,
        title: first.title.clone(),
        creator: first.creator.clone(),
        tags: serde_json::from_str(&first.tags).unwrap(),
        post_type: first.post_type,
        like_count: first.like_count,
        generated_title: first.generated_title.clone(),
        created_at: first
            .created_at
            .clone()
            .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()),
        links: posts
            .into_iter()
            .map(|post| PostLink {
                id: post.rowid,
                url: post.url,
                content_type: post.content_type,
                source: post.source,
                status: post.status,
                error: post.error,
                file_path: post.file_path,
                file_path_pattern: post.file_path_pattern,
            })
            .collect(),
    }
}

pub struct Database {
    db: SqlitePool,
}

impl Database {
    pub fn new(pool: SqlitePool) -> Self {
        Self { db: pool }
    }

    pub async fn insert_post(&self, post: &CreatePost) -> Result<()> {
        info!("Inserting post: {:#?}", post);
        let tags = serde_json::to_string(&post.tags)?;
        let mut transaction = self.db.begin().await?;
        sqlx::query!(
            "
            INSERT INTO posts (id, title, creator, tags, post_type, like_count)
            VALUES (?, ?, ?, ?, ?, ?)
        ",
            post.id,
            post.title,
            post.creator,
            tags,
            post.post_type,
            post.like_count,
        )
        .execute(&mut *transaction)
        .await?;

        for link in &post.links {
            sqlx::query!(
                "
                INSERT INTO post_links (url, content_type, source, post_id, status)
                VALUES (?, ?, ?, ?, ?)
            ",
                link.url,
                link.content_type,
                link.source,
                post.id,
                LinkStatus::Pending,
            )
            .execute(&mut *transaction)
            .await?;
        }

        transaction.commit().await?;

        Ok(())
    }

    pub async fn set_post_date(&self, post_id: i64, date: NaiveDate) -> Result<()> {
        let date = date.format("%Y-%m-%d").to_string();

        sqlx::query!(
            "UPDATE posts SET created_at = ? WHERE id = ?",
            date,
            post_id
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    pub async fn fetch_by_id(&self, id: i64) -> Result<Post> {
        let post = sqlx::query_as!(
            JoinedPost,
            "SELECT p.id, p.title, p.creator, p.tags, p.post_type, p.like_count, p.generated_title, p.created_at,
                   pl.rowid, pl.url, pl.content_type, pl.source, pl.status, pl.error, pl.file_path, pl.file_path_pattern
            FROM posts p
            INNER JOIN post_links pl ON p.id = pl.post_id 
            WHERE id = ?",
            id
        )
        .fetch_all(&self.db)
        .await?;
        Ok(to_hutt_post(post))
    }

    pub async fn reset_downloads(&self) -> Result<()> {
        sqlx::query!("UPDATE post_links SET status = 'pending', error = NULL, file_path = NULL, file_path_pattern = NULL")
            .execute(&self.db)
            .await?;
        Ok(())
    }

    pub async fn update_path(&self, link_id: i64, file_path: &str, pattern: &str) -> Result<()> {
        sqlx::query!(
            "UPDATE post_links SET file_path = ?, file_path_pattern = ? WHERE rowid = ?",
            file_path,
            pattern,
            link_id
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    pub async fn set_generated_title(&self, post_id: i64, title: &str) -> Result<()> {
        sqlx::query!(
            "UPDATE posts SET generated_title = ? WHERE id = ?",
            title,
            post_id
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    pub async fn fetch_all(&self) -> Result<Vec<Post>> {
        use itertools::Itertools;

        let posts = sqlx::query_as!(
            JoinedPost,
            "SELECT p.id, p.title, p.creator, p.tags, p.post_type, p.like_count, p.generated_title, p.created_at,
                   pl.rowid, pl.url, pl.content_type, pl.source, pl.status, pl.error, pl.file_path, pl.file_path_pattern
            FROM posts p INNER JOIN post_links pl ON p.id = pl.post_id
            ORDER BY p.id ASC"
        )
        .fetch_all(&self.db)
        .await?;

        let groups: BTreeMap<i64, Vec<JoinedPost>> = posts
            .into_iter()
            .chunk_by(|post| post.id)
            .into_iter()
            .map(|(id, group)| (id, group.collect_vec()))
            .collect();

        Ok(groups
            .into_iter()
            .map(|(_, posts)| to_hutt_post(posts))
            .collect())
    }

    pub async fn update_status(&self, link_id: i64, status_update: StatusUpdate) -> Result<()> {
        match status_update {
            StatusUpdate::Success {
                file_path,
                file_path_pattern,
            } => {
                sqlx::query!(
                    "UPDATE post_links SET status = 'downloaded', file_path = ?, file_path_pattern = ? WHERE rowid = ?",
                    file_path,
                    file_path_pattern,
                    link_id,
                )
                .execute(&self.db)
                .await?;
            }
            StatusUpdate::Error { error } => {
                sqlx::query!(
                    "UPDATE post_links SET status = 'error', error = ? WHERE rowid = ?",
                    error,
                    link_id
                )
                .execute(&self.db)
                .await?;
            }
            StatusUpdate::Pending => {
                sqlx::query!(
                    "UPDATE post_links SET status = 'pending' WHERE rowid = ?",
                    link_id
                )
                .execute(&self.db)
                .await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use color_eyre::Result;
    use fake::faker::lorem::en::{Sentence, Words};
    use fake::faker::name::en::Name;
    use fake::Fake;
    use rand::seq::SliceRandom;
    use rand::Rng;
    use sqlx::SqlitePool;

    use super::{CreatePost, CreatePostLink, LinkSource, PostType};
    use crate::database::Database;

    fn random_link_source() -> LinkSource {
        let mut rng = rand::thread_rng();
        [
            LinkSource::HtmlString,
            LinkSource::ImageGallery,
            LinkSource::VideoPost,
        ]
        .choose(&mut rng)
        .unwrap()
        .clone()
    }

    fn random_post_type() -> PostType {
        let mut rng = rand::thread_rng();
        [PostType::Image, PostType::Video]
            .choose(&mut rng)
            .unwrap()
            .clone()
    }

    fn random_links(min: u32, max: u32) -> Vec<CreatePostLink> {
        let mut rng = rand::thread_rng();
        let count = rng.gen_range(min..max);
        (0..count)
            .map(|_| CreatePostLink {
                url: format!("https://hutt.co/images/{}/big", rng.gen_range(1000..9999)),
                content_type: ["image/jpeg", "image/png", "video/mp4"]
                    .choose(&mut rng)
                    .unwrap()
                    .to_string(),
                source: random_link_source(),
            })
            .collect()
    }

    fn random_post() -> CreatePost {
        let tags: Vec<String> = Words(0..10).fake();

        CreatePost {
            id: (0..10_000).fake(),
            title: Sentence(5..10).fake(),
            creator: Name().fake(),
            tags,
            links: random_links(1, 10),
            post_type: random_post_type(),
            like_count: (0..250).fake(),
        }
    }

    #[sqlx::test]
    async fn test_insert_post(pool: SqlitePool) -> Result<()> {
        let database = Database::new(pool);
        let post = random_post();
        database.insert_post(&post).await?;

        let result = database.fetch_by_id(post.id).await?;
        assert_eq!(result.id, post.id);

        Ok(())
    }

    #[sqlx::test]
    async fn test_list_posts(pool: SqlitePool) -> Result<()> {
        let database = Database::new(pool);
        let mut expected = (0..10).map(|_| random_post()).collect::<Vec<_>>();

        expected.sort_by_key(|p| p.id);
        for post in &expected {
            database.insert_post(post).await?;
        }

        let result = database.fetch_all().await?;
        assert_eq!(result.len(), expected.len());

        Ok(())
    }

    #[sqlx::test]
    async fn test_set_file_path(pool: SqlitePool) -> Result<()> {
        let database = Database::new(pool);
        let post = random_post();
        database.insert_post(&post).await?;
        let post = database.fetch_by_id(post.id).await?;

        let link = post.links.first().unwrap();
        let new_path = format!("/tmp/{}", link.url);
        database.update_path(link.id, &new_path, "test").await?;

        let result = database.fetch_by_id(post.id).await?;
        let updated_link = result.links.first().unwrap();
        assert_eq!(updated_link.file_path, Some(new_path));
        assert_eq!(updated_link.file_path_pattern, Some("test".to_string()));

        Ok(())
    }
}
