use anyhow::{Context, Result};

const HN_ITEM_URL_PREFIX: &str = "https://news.ycombinator.com/item?id=";

pub fn safe_story_url(candidate: &str, story_id: i32) -> String {
    match reqwest::Url::parse(candidate) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => url.to_string(),
        _ => format!("{}{}", HN_ITEM_URL_PREFIX, story_id),
    }
}

pub fn open_story_url(candidate: &str, story_id: i32) -> Result<()> {
    let url = safe_story_url(candidate, story_id);
    open::that(&url).with_context(|| format!("Failed to open URL: {}", url))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::safe_story_url;

    #[test]
    fn keeps_http_and_https_urls() {
        assert_eq!(
            safe_story_url("https://example.com/path", 42),
            "https://example.com/path"
        );
        assert_eq!(
            safe_story_url("http://example.com/path", 42),
            "http://example.com/path"
        );
    }

    #[test]
    fn falls_back_for_non_web_or_malformed_urls() {
        assert_eq!(
            safe_story_url("file:///tmp/story", 42),
            "https://news.ycombinator.com/item?id=42"
        );
        assert_eq!(
            safe_story_url("mailto:hello@example.com", 42),
            "https://news.ycombinator.com/item?id=42"
        );
        assert_eq!(
            safe_story_url("not a url", 42),
            "https://news.ycombinator.com/item?id=42"
        );
    }
}
