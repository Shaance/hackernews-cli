use super::extract;

fn extract_fixture(html: &[u8], url: &str) -> String {
    extract(html, &url.parse().unwrap()).unwrap()
}

#[test]
fn github_releases_use_notes_instead_of_hidden_errors() {
    let text = extract_fixture(
        include_bytes!("../../../tests/fixtures/reader/github-release.html"),
        "https://github.com/jquery/jquery/releases/tag/4.1.0",
    );
    assert!(!text.contains("Sorry, something went wrong."));
    assert!(text.contains("Add the responseURL field to jqXHR"));
    assert!(text.contains("Increase test coverage of the serialize module"));
}

#[test]
fn cloudflare_blog_recovers_article_inside_main() {
    let text = extract_fixture(
        include_bytes!("../../../tests/fixtures/reader/cloudflare-blog.html"),
        "https://blog.cloudflare.com/quad9-withdrawal-from-france/",
    );
    assert!(text.contains("Last month, the Swiss non-profit foundation Quad9"));
    assert!(text.contains("We have grave concerns about the disruption"));
    assert!(!text.contains("Skip to content"));
}

#[test]
fn ordinary_blog_keeps_article_body() {
    let text = extract_fixture(
        include_bytes!("../../../tests/fixtures/reader/death-by-ai.html"),
        "https://nicolasbouliane.com/blog/death-by-ai",
    );
    assert!(text.starts_with("This is the traffic to All About Berlin"));
    assert!(text.contains("fits in that future."));
}
