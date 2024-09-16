use camino::{Utf8Path, Utf8PathBuf};

use crate::database::{Post, PostType};

fn is_smiley(token: &str) -> bool {
    token.starts_with(':') && token.len() == 2 || token.contains("<") || token.contains(">")
}

fn limit_length(mut input: Vec<String>, max_len: usize) -> String {
    let mut tokens = vec![];
    let mut len = 0;

    while len < max_len {
        if input.is_empty() {
            break;
        }
        let token = input.remove(0);
        len += token.len();
        tokens.push(token);
    }

    tokens.join(" ")
}

fn sanitize(input: &str) -> String {
    sanitize_filename::sanitize_with_options(
        input,
        sanitize_filename::Options {
            replacement: " ",
            ..Default::default()
        },
    )
}

fn ignored_tokens(t: &&str) -> bool {
    !is_smiley(t) && *t != "/" && *t != "/" && !t.starts_with("http")
}

fn fix_token(token: &str) -> String {
    token.replace("/", " ")
}

fn get_post_title(post: &Post) -> String {
    const MAX_LEN: usize = 50;

    let tokens = post
        .title
        .split_whitespace()
        .filter(ignored_tokens)
        .map(fix_token)
        .collect::<Vec<_>>();
    let title = limit_length(tokens.clone(), MAX_LEN);
    let result = if title.is_empty() {
        let tags = limit_length(post.tags.clone(), MAX_LEN);
        if tags.is_empty() {
            "no title".into()
        } else {
            tags
        }
    } else {
        title
    };

    result.trim().into()
}

pub fn get_download_path(
    post: &Post,
    link_id: i64,
    pattern: &str,
    base_dir: impl AsRef<Utf8Path>,
) -> Utf8PathBuf {
    let name = pattern
        .replace("{post_id}", &post.id.to_string())
        .replace("{title}", &get_post_title(post))
        .replace("{link_id}", &link_id.to_string())
        .replace(
            "{type}",
            match post.post_type {
                PostType::Video => "Videos",
                PostType::Image => "Images",
            },
        );

    let parts = name.split('/').map(|part| sanitize(part));
    let mut path = base_dir.as_ref().to_owned();
    for part in parts {
        path.push(part.trim());
    }
    let extension = match post.post_type {
        PostType::Video => "mp4",
        PostType::Image => "jpeg",
    };
    path.set_extension(extension);

    path
}

#[cfg(test)]
mod tests {
    use crate::database::{Post, PostType};

    const PATTERN_1: &str = "{type}/{post_id} - {title} - {link_id}";
    const PATTERN_2: &str = "{type}/{post_id} - {title}/{link_id}";
    const ROOT: &str = "./downloads";

    #[test]
    fn test_title_with_smiley() {
        let post = Post {
            id: 543321,
            title: "Hello :) :( <3 >.>".to_string(),
            tags: vec![],
            post_type: PostType::Image,
            links: vec![],
            creator: "".into(),
            like_count: 0,
            generated_title: None,
            created_at: None,
        };

        let title = super::get_download_path(&post, 12345, PATTERN_1, ROOT);
        assert_eq!(title.file_name().unwrap(), "543321 - Hello - 12345.jpeg");
    }

    #[test]
    fn test_long_title() {
        let post = Post {
            id: 543321,
            title: "Snapchat dump photos! So, snapchat is being unfair and won't let me save like the majorityh of my stories. I'm trying to figure it out )))):".to_string(),
            tags: vec![],
            post_type: PostType::Image,
            links: vec![],
            creator: "".into(),
            like_count: 0,
            generated_title: None,
            created_at: None,
        };

        let title = super::get_download_path(&post, 12345, PATTERN_1, ROOT);
        assert_eq!(
            title.file_name().unwrap(),
            "543321 - Snapchat dump photos! So, snapchat is being unfair and won't - 12345.jpeg"
        );
    }

    #[test]
    fn test_no_title() {
        let post = Post {
            id: 543321,
            title: "".to_string(),
            tags: ["tailplug", "boobs", "ass", "petplay", "collar", "pussy"]
                .into_iter()
                .map(ToOwned::to_owned)
                .collect(),
            post_type: PostType::Image,
            links: vec![],
            creator: "".into(),
            like_count: 0,
            generated_title: None,
            created_at: None,
        };

        let title = super::get_download_path(&post, 12345, PATTERN_1, ROOT);
        assert_eq!(
            title.file_name().unwrap(),
            "543321 - tailplug boobs ass petplay collar pussy - 12345.jpeg"
        );
    }

    #[test]
    fn test_title_with_dots() {
        let post = Post {
            id: 543321,
            tags: ["tailplug", "boobs", "ass", "petplay", "collar", "pussy"]
                .into_iter()
                .map(ToOwned::to_owned)
                .collect(),
            post_type: PostType::Image,
            links: vec![],
            creator: "".into(),
            like_count: 0,
            title: "presentingggggg..".to_string(),
            generated_title: None,
            created_at: None,
        };

        let title = super::get_download_path(&post, 1234, PATTERN_2, ROOT);
        assert_eq!(
            title,
            "./downloads/Images/543321 - presentingggggg/1234.jpeg"
        );
    }

    #[test]
    fn test_title_with_slash() {
        let post = Post {
            id: 543321,
            tags: vec![],
            post_type: PostType::Image,
            links: vec![],
            creator: "".into(),
            like_count: 0,
            title: "something / something else".to_string(),
            generated_title: None,
            created_at: None,
        };

        let title = super::get_download_path(&post, 1234, PATTERN_2, ROOT);
        assert_eq!(
            title,
            "./downloads/Images/543321 - something something else/1234.jpeg"
        );
    }

    #[test]
    fn test_title_with_slash_2() {
        let post = Post {
            id: 543321,
            tags: vec![],
            post_type: PostType::Image,
            links: vec![],
            creator: "".into(),
            like_count: 0,
            title: "something/something else".to_string(),
            generated_title: None,
            created_at: None,
        };

        let title = super::get_download_path(&post, 1234, PATTERN_2, ROOT);
        assert_eq!(
            title,
            "./downloads/Images/543321 - something something else/1234.jpeg"
        );
    }

    #[test]
    fn test_title_with_url() {
        let post = Post {
            id: 543321,
            tags: vec![],
            post_type: PostType::Image,
            links: vec![],
            creator: "".into(),
            like_count: 0,
            title: "My SFW question answers! https://beacons.ai/auroraflower".to_string(),
            generated_title: None,
            created_at: None,
        };

        let title = super::get_download_path(&post, 1234, PATTERN_2, ROOT);
        assert_eq!(
            title,
            "./downloads/Images/543321 - My SFW question answers!/1234.jpeg"
        );
    }
}
