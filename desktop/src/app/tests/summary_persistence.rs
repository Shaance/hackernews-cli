//! Persistence is exercised with fake providers and isolated state directories.

use super::super::*;
use super::support::*;

fn disk_app(dir: &std::path::Path) -> App {
    let mut app = app(true, SavedView::in_memory());
    app.summary_store = Some(SummaryStore::disk(dir.to_path_buf()));
    app
}

fn request_counts(app: &App) -> (usize, usize) {
    let Source::Fake(fake) = app.source.as_ref() else { panic!("fixture must use the fake provider") };
    (*fake.article_summary_requests.lock().unwrap(), *fake.summary_requests.lock().unwrap())
}

#[test]
fn restart_restores_both_halves_without_provider_requests_and_keeps_citations() {
    let dir = temp_dir("summary-restart");
    let _ = std::fs::remove_dir_all(&dir);
    let item = crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID);
    let mut first = disk_app(&dir);
    first.open_story(item.clone());
    assert_eq!(request_counts(&first), (1, 1));
    let original = first.story.as_ref().unwrap();
    let article = original.article_summary.readable().unwrap().to_string();
    let discussion = summary_contents(original.summary.readable().unwrap());
    drop(first);

    let mut restarted = disk_app(&dir);
    restarted.open_story(item);
    assert_eq!(request_counts(&restarted), (0, 0));
    let restored = restarted.story.as_ref().unwrap();
    assert!(matches!(restored.article_summary, crate::story::ArticleSummaryState::Done(_)));
    assert!(matches!(restored.summary, crate::story::SummaryState::Done(_)));
    assert_eq!(restored.article_summary.readable(), Some(article.as_str()));
    assert_eq!(summary_contents(restored.summary.readable().unwrap()), discussion);
    assert!(restored.restored_summary.is_some());
    restarted.open_citation(0);
    let source = restarted.story.as_ref().unwrap().source().expect("the restored citation opens");
    assert_eq!(source.captured().id, 901);
    assert!(!source.captured().text.is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn restart_requests_only_the_missing_half() {
    let dir = temp_dir("summary-partial");
    let _ = std::fs::remove_dir_all(&dir);
    let item = crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID);
    let mut fake = crate::service::fake::FakeService::fixture();
    fake.article_summary = Arc::new(|_| Box::pin(async {
        Err(crate::story_io::SummaryError::new("unavailable", false))
    }));
    let mut first = disk_app(&dir);
    first.source = Arc::new(Source::Fake(fake));
    first.open_story(item.clone());
    assert_eq!(request_counts(&first), (1, 1));
    let file: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join(format!("{}.json", item.id))).unwrap()).unwrap();
    assert!(file["article"].is_null());
    assert!(file["discussion"].is_object());
    drop(first);

    let mut restarted = disk_app(&dir);
    restarted.open_story(item);
    assert_eq!(request_counts(&restarted), (1, 0));
    let story = restarted.story.as_ref().unwrap();
    assert!(matches!(story.article_summary, crate::story::ArticleSummaryState::Done(_)));
    assert!(matches!(story.summary, crate::story::SummaryState::Done(_)));
    assert!(story.restored_summary.is_some(), "the restored discussion remains visible");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unreadable_discussion_is_shown_but_not_persisted() {
    use hn_lib::reader::Discussion;

    for case in ["no-comments", "empty-excerpt", "readable"] {
        let dir = temp_dir(&format!("summary-{case}"));
        let _ = std::fs::remove_dir_all(&dir);
        let mut fake = crate::service::fake::FakeService::fixture();
        match case {
            "no-comments" => fake.comments.lock().unwrap().clear(),
            "empty-excerpt" => {
                let mut comments = fake.comments.lock().unwrap();
                comments.truncate(1);
                // The capture accepts this comment but removes the control character.
                comments[0].text = "\u{1}".into();
            }
            _ => {}
        }
        fake.summary = Arc::new(|_| Box::pin(async {
            Ok(Discussion { overview: "No readable comments to summarize yet.".into(), points: Vec::new() })
        }));
        let mut app = disk_app(&dir);
        app.source = Arc::new(Source::Fake(fake));
        app.open_story(crate::fixtures::item(0, 1));
        assert_eq!(app.story.as_ref().unwrap().summary.readable().unwrap().overview,
            "No readable comments to summarize yet.");
        let file: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("1.json")).unwrap()).unwrap();
        assert_eq!(file["discussion"].is_object(), case == "readable", "{case}");
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn saved_line_tracks_the_half_still_displayed_after_a_partial_regeneration() {
    use crate::story_io::{Provenance, StoryUpdate, SummaryError};
    use hn_lib::reader::Article;

    for article_first in [true, false] {
        let mut seed = app(true, SavedView::in_memory());
        super::super::scenes::scene_summary_saved(&mut seed);
        let mut store = seed.summary_store.take().unwrap();
        let mut saved = store.load(crate::fixtures::STORY_ID).unwrap();
        saved.discussion.as_mut().unwrap().saved_at = 1_700_000_000 - 2 * 3600;
        store.write(&saved).unwrap();

        let mut app = app(true, SavedView::in_memory());
        app.summary_store = Some(store);
        app.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        app.story.as_mut().unwrap().retry(true);
        let update = |article: bool, success: bool| {
            let provenance = Provenance::from(&Config::default());
            if article {
                StoryUpdate::ArticleSummary {
                    story_id: crate::fixtures::STORY_ID,
                    summary: if success { Ok(Article { text: "Fresh article".into() }) }
                        else { Err(SummaryError::new("offline", false)) },
                    provenance, empty_capture: false,
                }
            } else {
                StoryUpdate::Summary {
                    story_id: crate::fixtures::STORY_ID,
                    summary: if success { Ok((crate::fixtures::summary(), crate::fixtures::sources())) }
                        else { Err(SummaryError::new("offline", false)) },
                    provenance,
                }
            }
        };
        assert!(app.land_story_update(update(article_first, true)));
        let mut rt = rune::Runtime::new(app, super::super::window::view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.frame(0.0);
        let expected = if article_first { "Saved 2h ago · 12 new comments" } else { "Saved 5h ago" };
        let status = rt.elements().into_iter().find(|element| element.key.as_deref() == Some("summary-saved") && !element.exiting)
            .and_then(|element| element.text).expect("the still-restored half keeps its saved status");
        assert!(status.starts_with(expected), "{status}");
        if !article_first { assert!(!status.contains("new comment")); }

        rt.update(|app, _| assert!(app.land_story_update(update(!article_first, false))));
        rt.frame(0.0);
        assert!(rt.elements().iter().any(|element| element.key.as_deref() == Some("summary-saved") && !element.exiting));
        rt.update(|app, _| {
            app.story.as_mut().unwrap().retry(true);
            assert!(app.land_story_update(update(!article_first, true)));
        });
        rt.frame(0.0);
        assert!(!rt.elements().iter().any(|element| element.key.as_deref() == Some("summary-saved") && !element.exiting));
    }
}

#[test]
fn failures_cancelled_results_and_empty_article_are_not_saved() {
    use crate::story_io::{Provenance, StoryUpdate, SummaryError};
    use hn_lib::reader::Article;

    for empty in [false, true] {
        let dir = temp_dir(if empty { "summary-empty" } else { "summary-errors" });
        let _ = std::fs::remove_dir_all(&dir);
        let mut fake = crate::service::fake::FakeService::fixture();
        if empty {
            fake.article = Arc::new(|_| Box::pin(async { Ok(("  \n".into(), false)) }));
            fake.article_summary = Arc::new(|_| Box::pin(async {
                Ok(Article { text: "No article text to summarize.".into() })
            }));
        } else {
            fake.article_summary = Arc::new(|_| Box::pin(async { Err(SummaryError::new("offline", false)) }));
        }
        fake.summary = Arc::new(|_| Box::pin(async { Err(SummaryError::new("offline", false)) }));
        let mut app = disk_app(&dir);
        app.source = Arc::new(Source::Fake(fake));
        app.open_story(crate::fixtures::item(0, 1));
        assert!(!dir.join("1.json").exists());
        assert!(!app.land_story_update(StoryUpdate::ArticleSummary {
            story_id: 99, summary: Ok(Article { text: "stale".into() }),
            provenance: Provenance::from(&Config::default()), empty_capture: false,
        }));
        assert!(!dir.join("99.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    let dir = temp_dir("summary-cancelled");
    let _ = std::fs::remove_dir_all(&dir);
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let mut fake = crate::service::fake::FakeService::fixture();
    let article_gate = gate.clone();
    fake.article_summary = Arc::new(move |_| {
        let gate = article_gate.clone();
        Box::pin(async move {
            gate.acquire().await.unwrap().forget();
            Ok(Article { text: "cancelled article".into() })
        })
    });
    let discussion_gate = gate.clone();
    fake.summary = Arc::new(move |_| {
        let gate = discussion_gate.clone();
        Box::pin(async move {
            gate.acquire().await.unwrap().forget();
            Ok(crate::fixtures::summary())
        })
    });
    let mut app = app(false, SavedView::in_memory());
    app.summary_store = Some(SummaryStore::disk(dir.clone()));
    app.source = Arc::new(Source::Fake(fake));
    app.config.api_key = "fixture".into();
    app.open_story(crate::fixtures::item(0, 1));
    poll_until(&mut app, |app| request_counts(app) == (1, 1));
    app.story.as_mut().unwrap().stop_summaries();
    gate.add_permits(2);
    poll_until(&mut app, |app| !app.bridge.is_story_pending());
    assert!(!dir.join("1.json").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn regeneration_replaces_successful_halves_and_keeps_failed_ones() {
    use hn_lib::reader::{Article, Profile, Provider};
    use crate::story_io::SummaryError;

    let dir = temp_dir("summary-regenerate");
    let _ = std::fs::remove_dir_all(&dir);
    let mut app = disk_app(&dir);
    app.open_story(crate::fixtures::item(0, 1));
    let path = dir.join("1.json");
    let original: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();

    let mut fake = crate::service::fake::FakeService::fixture();
    fake.article_summary = Arc::new(|_| Box::pin(async { Ok(Article { text: "Fresh article".into() }) }));
    fake.summary = Arc::new(|_| Box::pin(async { Err(SummaryError::new("offline", false)) }));
    app.source = Arc::new(Source::Fake(fake));
    app.config.provider = Provider::Anthropic;
    app.config.profile = Profile::Thorough;
    app.retry_summary();
    let updated: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(updated["article"]["text"], "Fresh article");
    assert_eq!(updated["article"]["provider"], "Anthropic");
    assert_eq!(updated["article"]["model"], Profile::Thorough.model(Provider::Anthropic));
    assert_eq!(updated["discussion"], original["discussion"]);
    assert!(matches!(app.story.as_ref().unwrap().summary, crate::story::SummaryState::Failed { .. }));
    assert!(app.story.as_ref().unwrap().summary.readable().is_some());

    let mut failing = crate::service::fake::FakeService::fixture();
    failing.article_summary = Arc::new(|_| Box::pin(async { Err(SummaryError::new("offline", false)) }));
    failing.summary = Arc::new(|_| Box::pin(async { Err(SummaryError::new("offline", false)) }));
    app.source = Arc::new(Source::Fake(failing));
    app.retry_summary();
    let after_failure: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(after_failure, updated);
    assert_eq!(app.story.as_ref().unwrap().article_summary.readable(), Some("Fresh article"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn bad_and_unknown_files_are_ignored_then_overwritten() {
    for (name, bytes) in [("corrupt", b"{".as_slice()), ("version", br#"{"version":99,"story_id":1,"article":null,"discussion":null}"#.as_slice())] {
        let dir = temp_dir(&format!("summary-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("1.json"), bytes).unwrap();
        let mut app = disk_app(&dir);
        app.open_story(crate::fixtures::item(0, 1));
        assert_eq!(request_counts(&app), (1, 1));
        let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("1.json")).unwrap()).unwrap();
        assert_eq!(saved["version"], 1);
        assert!(saved["article"].is_object() && saved["discussion"].is_object());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn provenance_is_captured_when_each_request_starts() {
    use hn_lib::reader::{Article, Profile, Provider};
    let dir = temp_dir("summary-provenance");
    let _ = std::fs::remove_dir_all(&dir);
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let mut fake = crate::service::fake::FakeService::fixture();
    let article_gate = gate.clone();
    fake.article_summary = Arc::new(move |_| {
        let gate = article_gate.clone();
        Box::pin(async move {
            gate.acquire().await.unwrap().forget();
            Ok(Article { text: "article".into() })
        })
    });
    let discussion_gate = gate.clone();
    fake.summary = Arc::new(move |_| {
        let gate = discussion_gate.clone();
        Box::pin(async move {
            gate.acquire().await.unwrap().forget();
            Ok(crate::fixtures::summary())
        })
    });
    let mut app = app(false, SavedView::in_memory());
    app.summary_store = Some(SummaryStore::disk(dir.clone()));
    app.source = Arc::new(Source::Fake(fake));
    app.config.api_key = "fixture".into();
    app.open_story(crate::fixtures::item(0, 1));
    poll_until(&mut app, |app| request_counts(app) == (1, 1));
    app.config.provider = Provider::Anthropic;
    app.config.profile = Profile::Thorough;
    gate.add_permits(2);
    poll_until(&mut app, |app| !app.bridge.is_story_pending());
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("1.json")).unwrap()).unwrap();
    for part in ["article", "discussion"] {
        assert_eq!(saved[part]["provider"], "OpenRouter");
        assert_eq!(saved[part]["profile"], "Fast");
        assert_eq!(saved[part]["model"], Profile::Fast.model(Provider::OpenRouter));
    }
    assert!(!saved.to_string().contains("fixture"), "credentials never enter the store");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn headless_construction_does_not_touch_state_directory() {
    const CHILD: &str = "HN_TEST_HEADLESS_SUMMARIES";
    if std::env::var_os(CHILD).is_none() {
        let dir = temp_dir("headless-summary-isolation");
        let _ = std::fs::remove_dir_all(&dir);
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "app::tests::summary_persistence::headless_construction_does_not_touch_state_directory", "--nocapture"])
            .env(CHILD, "1")
            .env("XDG_STATE_HOME", &dir)
            .env("XDG_CONFIG_HOME", &dir)
            .env("XDG_DATA_HOME", &dir)
            .env_remove("OPENROUTER_API_KEY")
            .env_remove("ANTHROPIC_API_KEY")
            .output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert!(!dir.exists(), "headless construction must not access the state directory");
    } else {
        let app = init_with_headless(true);
        assert!(app.summary_store.is_none());
    }
}

#[test]
fn regenerate_link_is_a_button_that_requests_fresh_summaries() {
    let mut s = app(true, SavedView::in_memory());
    super::super::scenes::scene_summary_saved(&mut s);
    assert_eq!(request_counts(&s), (0, 0), "both restored halves skip the provider");
    let mut rt = rune::Runtime::new(s, super::super::window::view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.frame(0.0);
    let link = rt.elements().iter().find(|e| e.key.as_deref() == Some("regenerate")).unwrap().clone();
    assert!(link.role == "button" && link.label.as_deref() == Some("Regenerate summary"));
    let [x, y, w, h] = link.rect;
    rt.pointer_down(x + w / 2.0, y + h / 2.0); rt.pointer_up(x + w / 2.0, y + h / 2.0); rt.frame(0.0);
    assert_eq!(request_counts(&rt.state), (1, 1));
}

#[test]
fn regenerate_link_is_withheld_while_generating_and_opens_settings_without_a_key() {
    use crate::story_io::{Provenance, StoryUpdate};
    use hn_lib::reader::Article;

    for case in ["no-key", "article-generating", "discussion-generating", "both-generating"] {
        let mut app = app(case != "no-key", SavedView::in_memory());
        super::super::scenes::scene_summary_saved(&mut app);
        if case != "no-key" {
            app.story.as_mut().unwrap().retry(true);
            let provenance = Provenance::from(&Config::default());
            if case == "article-generating" {
                assert!(app.land_story_update(StoryUpdate::Summary {
                    story_id: crate::fixtures::STORY_ID,
                    summary: Ok((crate::fixtures::summary(), crate::fixtures::sources())), provenance,
                }));
            } else if case == "discussion-generating" {
                assert!(app.land_story_update(StoryUpdate::ArticleSummary {
                    story_id: crate::fixtures::STORY_ID,
                    summary: Ok(Article { text: "Fresh article".into() }), provenance, empty_capture: false,
                }));
            }
        }
        let mut rt = rune::Runtime::new(app, super::super::window::view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.frame(0.0);
        let elements = rt.elements();
        let status = elements.iter().find(|element| element.key.as_deref() == Some("summary-saved"))
            .and_then(|element| element.text.as_deref()).expect("saved status remains visible").to_string();
        let link = elements.iter().find(|element| element.key.as_deref() == Some("regenerate")).cloned();
        if case == "no-key" {
            let [x, y, w, h] = link.expect("no-key: the link stays").rect;
            rt.pointer_down(x + w / 2.0, y + h / 2.0); rt.pointer_up(x + w / 2.0, y + h / 2.0); rt.frame(0.0);
            assert!(rt.state.settings.is_some(), "no-key: the link opens summary settings");
            assert!(rt.state.story.as_ref().unwrap().article_summary.readable().is_some(), "no-key: restored text stays");
            assert_eq!(request_counts(&rt.state), (0, 0));
        } else {
            assert!(link.is_none() && !status.contains("Regenerate"), "{case}: {status}");
        }
    }
}

#[test]
fn restored_halves_survive_automatic_off_and_failed_fetches() {
    let item = crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID);
    for case in ["automatic-off", "comments-fail", "article-fail"] {
        let dir = temp_dir(&format!("summary-survive-{case}"));
        let _ = std::fs::remove_dir_all(&dir);
        let mut first = disk_app(&dir);
        first.open_story(item.clone());
        let article = first.story.as_ref().unwrap().article_summary.readable().unwrap().to_string();
        let discussion = summary_contents(first.story.as_ref().unwrap().summary.readable().unwrap());
        drop(first);

        let mut fake = crate::service::fake::FakeService::fixture();
        match case {
            "comments-fail" => { fake.failing_comments.lock().unwrap().insert(crate::fixtures::STORY_ID); }
            "article-fail" => fake.article = Arc::new(|_| Box::pin(async { Err("page unavailable".to_string()) })),
            _ => {}
        }
        let mut restarted = disk_app(&dir);
        restarted.source = Arc::new(Source::Fake(fake));
        restarted.config.automatic = case != "automatic-off";
        restarted.open_story(item.clone());
        assert_eq!(request_counts(&restarted), (0, 0), "{case}");
        let restored = restarted.story.as_ref().unwrap();
        assert_eq!(restored.article_summary.readable(), Some(article.as_str()), "{case}");
        assert_eq!(summary_contents(restored.summary.readable().expect(case)), discussion, "{case}");
        assert!(restored.restored_summary.is_some(), "{case}: the saved line stays");
        match case {
            "comments-fail" => assert!(matches!(restored.discussion, Some(Err(_))), "the comments fetch failed"),
            "article-fail" => assert!(matches!(restored.article, crate::story::ArticleState::Failed(_)), "the article fetch failed"),
            _ => assert!(!restarted.config.automatic),
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
