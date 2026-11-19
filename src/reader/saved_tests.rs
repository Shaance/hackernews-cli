use super::*;
use serde_json::{json, Value};

pub(in crate::reader) fn temp_dir() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("hn-summary-{}-{suffix}", std::process::id()));
    fs::create_dir(&path).unwrap();
    path
}

pub(in crate::reader) fn article(saved_at: u64) -> Record<ArticleData> {
    let mut header = Header::new(1, Provider::Anthropic, Profile::Thorough);
    header.saved_at = saved_at;
    Record {
        header,
        data: ArticleData {
            article: Article {
                text: "Saved article".into(),
            },
            partial: true,
        },
    }
}

pub(in crate::reader) fn discussion(saved_at: u64) -> Record<DiscussionData> {
    let mut header = Header::new(1, Provider::OpenRouter, Profile::Fast);
    header.saved_at = saved_at;
    Record {
        header,
        data: DiscussionData {
            discussion: super::super::tests::discussion("Saved disagreement."),
            context: DiscussionContext {
                comments_at_save: 2,
                coverage: "2 excerpts (partial)".into(),
                sources: super::super::tests::discussion_capture().sources,
            },
        },
    }
}

#[test]
fn records_round_trip_every_field_with_independent_writers() {
    let root = temp_dir();
    let mut first = SummaryStore::disk(root.clone());
    let mut second = SummaryStore::disk(root.clone());
    let article = article(123);
    let discussion = discussion(789);
    let article_json = json!(article);
    let discussion_json = json!(discussion);
    assert_eq!(
        article_json,
        json!({
            "version":1, "story_id":1, "saved_at":123, "provider":"anthropic",
            "model": Profile::Thorough.model(Provider::Anthropic), "profile":"thorough",
            "article":{"text":"Saved article"}, "partial":true,
        })
    );
    assert_eq!(
        discussion_json,
        json!({
            "version":1, "story_id":1, "saved_at":789, "provider":"open_router",
            "model":Profile::Fast.model(Provider::OpenRouter), "profile":"fast",
            "comments_at_save":2, "coverage":"2 excerpts (partial)",
            "discussion":{"overview":"Two viewpoints.", "points":[{"text":"Saved disagreement.","sources":[11]}]},
            "sources":[
                {"id":10,"parent":null,"author":"parent","text":"Parent argument.","excerpt":"Parent argument."},
                {"id":11,"parent":10,"author":"reply","text":"Reply evidence.","excerpt":"Reply evidence."},
            ],
        })
    );
    first.save_article(article, false).unwrap();
    second.save_discussion(discussion).unwrap();

    for (name, expected) in [
        ("1.article.json", &article_json),
        ("1.discussion.json", &discussion_json),
    ] {
        let value: Value = serde_json::from_slice(&fs::read(root.join(name)).unwrap()).unwrap();
        assert_eq!(&value, expected);
    }
    let restarted = SummaryStore::disk(root.clone());
    assert_eq!(json!(restarted.load_article(1).unwrap()), article_json);
    assert_eq!(
        json!(restarted.load_discussion(1).unwrap()),
        discussion_json
    );
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn records_and_directory_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let parent = temp_dir();
    let root = parent.join("summaries");
    let mut store = SummaryStore::disk(root.clone());
    store.save_article(article(now()), false).unwrap();
    store.save_discussion(discussion(now())).unwrap();
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&root), 0o700);
    assert_eq!(mode(&root.join("1.article.json")), 0o600);
    assert_eq!(mode(&root.join("1.discussion.json")), 0o600);
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn prune_removes_only_expired_valid_halves() {
    let root = temp_dir();
    let mut store = SummaryStore::disk(root.clone());
    let old = now() - RETENTION - 60;
    let recent = now() - RETENTION + 60;
    store.save_article(article(old), false).unwrap();
    store.save_discussion(discussion(recent)).unwrap();
    let mut fresh_article = article(recent);
    fresh_article.header.story_id = 2;
    store.save_article(fresh_article, false).unwrap();
    let mut old_discussion = discussion(old);
    old_discussion.header.story_id = 2;
    store.save_discussion(old_discussion).unwrap();
    fs::write(root.join("3.article.json"), b"{").unwrap();
    fs::write(root.join(".1.article.json-4-1.tmp"), b"temporary").unwrap();
    store.prune();
    assert!(!root.join("1.article.json").exists());
    assert!(!root.join("2.discussion.json").exists());
    assert!(store.load_discussion(1).is_some());
    assert!(store.load_article(2).is_some());
    assert!(root.join("3.article.json").exists());
    assert!(root.join(".1.article.json-4-1.tmp").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn legacy_single_file_is_ignored_and_left_in_place() {
    let root = temp_dir();
    let path = root.join("1.json");
    fs::write(
        &path,
        br#"{"version":1,"story_id":1,"article":{"saved_at":1,"text":"old"},"discussion":null}"#,
    )
    .unwrap();
    let store = SummaryStore::disk(root.clone());
    assert!(store.load_article(1).is_none());
    assert!(store.load_discussion(1).is_none());
    store.prune();
    assert!(path.is_file());
    fs::remove_dir_all(root).unwrap();
}
