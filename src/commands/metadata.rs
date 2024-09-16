use color_eyre::Result;
use regex::Regex;
use reqwest::StatusCode;
use scraper::{ElementRef, Selector};
use serde::Deserialize;
use tracing::{info, warn};

use crate::database::{CreatePost, CreatePostLink, LinkSource, PostType};
use crate::DownloadContext;

pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

pub struct MetadataArgs {
    pub creator_name: String,
    pub creator_id: i64,
    pub cookie: String,
}

#[derive(Deserialize)]
struct GalleryImage {
    src: Option<String>,
    html: Option<String>,
}

struct UrlExtractor {}

impl UrlExtractor {
    fn parse_url_from_html(&self, html: &str) -> Option<CreatePostLink> {
        let re = Regex::new(r#"src="(.*?)""#).unwrap();
        if let Some(captures) = re.captures(html) {
            let path = captures.get(1).unwrap().as_str().to_string();
            return Some(CreatePostLink {
                url: path,
                content_type: "video/mp4".to_string(),
                source: LinkSource::HtmlString,
            });
        } else {
            None
        }
    }

    fn extract_urls(&self, element: ElementRef, post_type: PostType) -> Vec<CreatePostLink> {
        match post_type {
            PostType::Image => {
                let selector = Selector::parse("script").unwrap();
                let script_el = element.select(&selector).next().unwrap().inner_html();
                let re = Regex::new(r#"dynamicEl:\s+(.*),"#).unwrap();
                if let Some(captures) = re.captures(&script_el) {
                    let gallery_json = captures.get(1).unwrap().as_str().replace("\\>", " ");
                    match serde_json::from_str::<Vec<GalleryImage>>(&gallery_json) {
                        Ok(json) => {
                            let mut post_links = Vec::new();
                            for image in json {
                                if let Some(src) = image.src {
                                    post_links.push(CreatePostLink {
                                        url: src,
                                        content_type: "image/jpeg".to_string(),
                                        source: LinkSource::ImageGallery,
                                    });
                                }
                                if let Some(url) =
                                    image.html.and_then(|html| self.parse_url_from_html(&html))
                                {
                                    post_links.push(url);
                                }
                            }
                            return post_links;
                        }
                        Err(e) => {
                            warn!("failed to parse gallery json: {gallery_json}: {e:?}");
                            return Vec::new();
                        }
                    }
                } else {
                    warn!(
                        "failed to find gallery json in script element {}",
                        script_el
                    );
                    return Vec::new();
                }
            }
            PostType::Video => {
                let selector = Selector::parse("video source").unwrap();

                if let Some(source_element) = element.select(&selector).next() {
                    return vec![CreatePostLink {
                        url: source_element.attr("src").unwrap().to_string(),
                        content_type: "video/mp4".to_string(),
                        source: LinkSource::VideoPost,
                    }];
                } else {
                    warn!("failed to find video source element");
                    return Vec::new();
                }
            }
        }
    }
}

enum FetchResult {
    RateLimited,
    Posts(Vec<CreatePost>),
}

struct Selectors {
    post_wrapper: Selector,
    like_count: Selector,
    title: Selector,
    tags: Selector,
    video_element: Selector,
    image_element: Selector,
}

impl Selectors {
    fn new() -> Self {
        Self {
            post_wrapper: Selector::parse(".huttPost.has-media").unwrap(),
            like_count: Selector::parse(".likes-count").unwrap(),
            title: Selector::parse(".post-text").unwrap(),
            tags: Selector::parse(".tags a.label").unwrap(),
            video_element: Selector::parse("figure.hutt-video").unwrap(),
            image_element: Selector::parse(".img-responsive").unwrap(),
        }
    }
}

struct PostFetcher {
    context: DownloadContext,
    args: MetadataArgs,
    selectors: Selectors,
    url_extractor: UrlExtractor,
}

impl PostFetcher {
    fn extract_post_type(&self, element: ElementRef) -> Option<PostType> {
        let video = element.select(&self.selectors.video_element).next();
        if video.is_some() {
            return Some(PostType::Video);
        }
        let image = element.select(&self.selectors.image_element).next();
        if image.is_some() {
            return Some(PostType::Image);
        }
        None
    }

    fn extract_title(&self, element: ElementRef) -> String {
        let text: Option<String> = element
            .select(&self.selectors.title)
            .next()
            .map(|e| e.text().collect());
        text.unwrap_or_else(|| "Untitled".into())
    }

    fn extract_tags(&self, element: ElementRef) -> Vec<String> {
        let elements = element.select(&self.selectors.tags);
        let mut tags = vec![];
        for tag_el in elements {
            let tag: String = tag_el.text().collect();
            let tag = tag.trim().to_string();
            if !tag.is_empty() {
                if tag.starts_with("#") {
                    tags.push(tag[1..].to_string());
                } else {
                    tags.push(tag);
                }
            }
        }

        tags
    }

    fn scrape_posts(&self, text: String, creator_name: &str) -> Result<Vec<CreatePost>> {
        let document = scraper::Html::parse_document(&text);

        let mut posts = Vec::new();

        for element in document.select(&self.selectors.post_wrapper) {
            if let Some(id) = element.attr("id") {
                let id = id.replace("post-", "");
                let id: i64 = id.parse()?;
                info!("Scraping post {id}");
                let post_type = self.extract_post_type(element);
                if post_type.is_none() {
                    warn!("No post type found for post {id}, skipping");
                    continue;
                }
                let post_type = post_type.unwrap();
                let links = self.url_extractor.extract_urls(element, post_type);
                if links.is_empty() {
                    info!("No links found for post {id}, skipping");
                    continue;
                } else {
                    info!("Found {} links for post {id}", links.len());
                }
                let title = self.extract_title(element);
                let tags = self.extract_tags(element);
                let like_count: Option<String> = element
                    .select(&self.selectors.like_count)
                    .next()
                    .map(|e| e.text().collect());
                let like_count: i64 = like_count.and_then(|s| s.parse().ok()).unwrap_or_default();

                posts.push(CreatePost {
                    id,
                    like_count,
                    post_type,
                    tags: tags,
                    links,
                    title,
                    creator: creator_name.to_string(),
                })
            } else {
                info!("No id found for post, skipping");
            }
        }

        Ok(posts)
    }

    async fn fetch_posts(&self, page: u32) -> Result<FetchResult> {
        let creator_id = self.args.creator_id;
        let creator_name = &self.args.creator_name;
        info!("Fetching posts for creator {creator_name} ({creator_id}), page {page}");

        let url = format!("https://hutt.co/hutts/ajax-posts?page={page}&view=view&id={creator_id}");
        let response = self
            .context
            .client
            .get(&url)
            .header("Cookie", &self.args.cookie)
            .header("User-Agent", USER_AGENT)
            .send()
            .await?;
        if response.status() == StatusCode::TOO_MANY_REQUESTS {
            return Ok(FetchResult::RateLimited);
        } else {
            let text = response.text().await?;
            let posts = self.scrape_posts(text, creator_name)?;
            Ok(FetchResult::Posts(posts))
        }
    }

    async fn run(&self) -> Result<()> {
        use tokio::time;

        let mut page = 0;
        loop {
            let posts = self.fetch_posts(page).await?;
            match posts {
                FetchResult::RateLimited => {
                    warn!("Rate limited, sleeping for 2 minutes");
                    time::sleep(std::time::Duration::from_secs(120)).await;
                    continue;
                }
                FetchResult::Posts(posts) => {
                    if posts.is_empty() {
                        info!("No more posts found, stopping");
                        break;
                    }
                    for post in &posts {
                        self.context.database.insert_post(post).await?;
                    }
                    page += 1;
                }
            }
        }

        Ok(())
    }
}

pub async fn run(context: DownloadContext, args: MetadataArgs) -> Result<()> {
    let creator = PostFetcher {
        context,
        args,
        selectors: Selectors::new(),
        url_extractor: UrlExtractor {},
    };

    creator.run().await
}
