//! Summary generation, retry and result handoffs.

use super::super::*;
use std::time::Duration;
use rune::AppSpec;
use rune::Color;
use rune::Tx;
use crate::story::SummaryState;
use super::super::input::on_key;
use super::super::poll::poll;
use super::super::scenes::scene_story;
use super::super::window::view;
use super::support::*;

fn button_style(elements: &[rune::ElementInfo], label: &str) -> (f32, Color, Color) {
    let (index, button) = elements.iter().enumerate().find(|(_, e)| e.label.as_deref() == Some(label)).unwrap();
    let ink = elements.iter().find(|e| e.parent == Some(index) && e.key.as_deref() == Some("label"))
        .unwrap().text_color.unwrap();
    (button.rect[3], button.fill, ink)
}

#[test]
fn retry_controls_share_quiet_style_except_primary_summary_retry() {
    let mut feed = app(true, SavedView::in_memory());
    feed.feed.error = Some("offline".into());
    let mut rt = rune::Runtime::new(feed, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.frame(0.0);
    let quiet = button_style(&rt.elements(), "Retry");

    let mut s = app(true, SavedView::in_memory());
    scene_story(&mut s);
    let story = s.story.as_mut().unwrap();
    story.article_summary = crate::story::ArticleSummaryState::Failed { message: "offline".into(), auth: false, previous: None };
    story.summary = SummaryState::Failed { message: "offline".into(), auth: false, previous: None };
    story.discussion = Some(Err("offline".into()));
    let mut comments = crate::fixtures::thread();
    comments.iter_mut().find(|c| c.id == 901).unwrap().child_ids.push(987655);
    story.thread = crate::comments::Thread::from_comments(&comments);
    story.thread.merge_replies(901, &[987655], Err("offline".into()));
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.frame(0.0);
    let elements = rt.elements();
    for label in ["Retry summaries", "Retry discussion", "Retry replies (1)"] {
        assert_eq!(button_style(&elements, label), quiet, "{label}");
    }
    assert_ne!(button_style(&elements, "Retry").1, quiet.1, "summary retry is primary");
}

#[test]
fn quiet_button_text_meets_contrast_on_rendered_surfaces() {
    for dark in [false, true] {
        let mut s = app(true, SavedView::in_memory());
        scene_story(&mut s);
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(dark, 1.0));
        rt.frame(0.0);
        let elements = rt.elements();
        let ink = button_style(&elements, "Hide summary").2.luminance();
        for key in ["summary-card", "discussion-header"] {
            let bg = elements.iter().find(|e| e.key.as_deref() == Some(key)).unwrap().fill.luminance();
            let ratio = (ink.max(bg) + 0.05) / (ink.min(bg) + 0.05);
            assert!(ratio >= 4.5, "{key} dark={dark}: {ratio:.2}:1");
        }
    }
}

#[test]
fn summary_overflow_remains_keyboard_scrollable_and_leaves_discussion_room() {
    let mut s = app(true, SavedView::in_memory());
    s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
    s.story.as_mut().unwrap().article_summary = crate::story::ArticleSummaryState::Done("A long article synopsis. ".repeat(100));
    let mut rt = rune::Runtime::new(s, view, (1040.0, 640.0), rune::Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    let before = rt.elements();
    let summary = before.iter().find(|e| e.key.as_deref() == Some("summary-scroll")).unwrap();
    let discussion = before.iter().find(|e| e.key.as_deref() == Some("discussion-scroll")).unwrap();
    assert!(summary.focusable && summary.rect[3] <= 274.0);
    assert!(discussion.rect[3] > 200.0, "discussion remains usable: {:?}", discussion.rect);
    assert!(before.iter().find(|e| e.key.as_deref() == Some("summary-more")).unwrap().opacity > 0.9,
        "overflow is signalled without covering the content");
    let more = before.iter().find(|e| e.key.as_deref() == Some("summary-more")).unwrap();
    assert!(more.rect[1] >= summary.rect[1] + summary.rect[3] - 1.0);
    rt.update(|s, _| { s.story.as_mut().unwrap().thread.next(); });
    rt.frame(0.0);
    assert!(rt.elements().iter().find(|e| e.key.as_deref() == Some("summary-more")).unwrap().opacity > 0.9,
        "rebuilding the view preserves the overflow cue");
    let y = before.iter().find(|e| e.key.as_deref() == Some("article-summary")).unwrap().rect[1];
    route(&mut rt, rune::Key::Text("J".into()));
    let moved = rt.elements().iter().find(|e| e.key.as_deref() == Some("article-summary")).unwrap().rect[1];
    assert!(moved < y, "Shift+J scrolls the summary");
    rt.update(|_, tx| tx.focus("summary-scroll", true));
    rt.frame(0.0);
    route(&mut rt, rune::Key::PageUp);
    let restored = rt.elements().iter().find(|e| e.key.as_deref() == Some("article-summary")).unwrap().rect[1];
    assert!(restored > moved, "Page Up in the summary scrolls that viewport");
    rt.update(|_, tx| tx.scroll_to_instant("summary-scroll", 0.0, f32::MAX));
    rt.frame(0.0);
    assert!(rt.elements().iter().find(|e| e.key.as_deref() == Some("summary-more")).unwrap().opacity < 0.1,
        "the cue disappears at the end");
}

fn gated_summary_fixture() -> (App, tokio::sync::oneshot::Sender<()>, tokio::sync::oneshot::Sender<()>) {
    use std::sync::Mutex;
    use tokio::sync::oneshot;

    let (article_tx, article_rx) = oneshot::channel::<()>();
    let (discussion_tx, discussion_rx) = oneshot::channel::<()>();
    let article_rx = Mutex::new(Some(article_rx));
    let discussion_rx = Mutex::new(Some(discussion_rx));
    let mut fake = crate::service::fake::FakeService::fixture();
    fake.article_summary = Arc::new(move |_| {
        let gate = article_rx.lock().unwrap().take().unwrap();
        Box::pin(async move {
            gate.await.unwrap();
            Ok(hn_lib::reader::Article { text: "The article summary arrived.".into() })
        })
    });
    fake.summary = Arc::new(move |_| {
        let gate = discussion_rx.lock().unwrap().take().unwrap();
        Box::pin(async move { gate.await.unwrap(); Ok(crate::fixtures::summary()) })
    });
    let mut s = app(false, SavedView::in_memory());
    s.source = Arc::new(Source::Fake(fake));
    s.config.api_key = "fixture".into();
    s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
    poll_until(&mut s, |s| {
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        *fake.article_summary_requests.lock().unwrap() == 1 && *fake.summary_requests.lock().unwrap() == 1
    });
    s.live = Live::Failed { story_id: crate::fixtures::STORY_ID, error: "fixture".into() };
    (s, article_tx, discussion_tx)
}

#[test]
fn independent_summary_arrivals_move_only_the_ready_section() {
    for reduced in [false, true] {
        let (s, article_tx, discussion_tx) = gated_summary_fixture();
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.poll = Some(poll);
        rt.set_system_reduced_motion(reduced);
        rt.frame(0.0);

        let element = |rt: &rune::Runtime<App>, key: &str| rt.elements().into_iter()
            .find(|e| e.key.as_deref() == Some(key) && !e.exiting).unwrap();
        let wait_for = |rt: &mut rune::Runtime<App>, article: bool| {
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            while if article {
                !matches!(rt.state.story.as_ref().unwrap().article_summary, crate::story::ArticleSummaryState::Done(_))
            } else {
                !matches!(rt.state.story.as_ref().unwrap().summary, SummaryState::Done(_))
            } {
                rt.frame(0.0);
                assert!(std::time::Instant::now() < deadline, "summary result did not land");
                std::thread::yield_now();
            }
        };
        article_tx.send(()).unwrap();
        wait_for(&mut rt, true);
        rt.frame(0.05);
        let article_50 = element(&rt, "article-summary-result");
        let discussion = element(&rt, "discussion-summary-result");
        rt.frame(0.05);
        let article_100 = element(&rt, "article-summary-result");
        if reduced {
            assert_eq!((article_50.opacity, article_100.opacity), (1.0, 1.0));
            assert_eq!(article_50.rect, article_50.target);
        } else {
            assert!(0.4 < article_50.opacity && article_50.opacity < article_100.opacity && article_100.opacity < 1.0,
                "article at 50/100ms: {} {}", article_50.opacity, article_100.opacity);
            assert!(discussion.opacity < 1.0, "pending discussion must not enter with the article");
        }
        assert!(!rt.elements().iter().any(|e| e.exiting && e.text.is_some() && e.opacity > 0.001
            && e.path.contains("summary-scroll")), "outgoing loading text must not overlap");

        discussion_tx.send(()).unwrap();
        wait_for(&mut rt, false);
        assert!(rt.state.toast.is_none(), "a visible discussion result needs no toast");
        rt.frame(0.05);
        let discussion_50 = element(&rt, "discussion-summary-result");
        rt.frame(0.05);
        let discussion_100 = element(&rt, "discussion-summary-result");
        if reduced {
            assert_eq!((discussion_50.opacity, discussion_100.opacity), (1.0, 1.0));
            assert_eq!(discussion_50.rect, discussion_50.target);
        } else {
            assert!(0.4 < discussion_50.opacity && discussion_50.opacity < discussion_100.opacity && discussion_100.opacity < 1.0,
                "discussion at 50/100ms: {} {}", discussion_50.opacity, discussion_100.opacity);
        }
    }
}

#[test]
fn a_folded_summary_still_announces_its_result() {
    let (mut s, article, discussion) = gated_summary_fixture();
    s.story.as_mut().unwrap().summary_open = false;
    discussion.send(()).unwrap();
    poll_until(&mut s, |s| matches!(s.story.as_ref().unwrap().summary, SummaryState::Done(_)));
    assert_eq!(s.toast.as_ref().map(|notice| notice.message.as_str()), Some("Discussion summary ready"));
    drop(s);
    drop(article);
}

#[test]
#[ignore = "Explicit Metal-backed captures of the actual async summary handoff"]
fn render_summary_arrival_frames() {
    use std::cell::RefCell;
    use tokio::sync::oneshot::Sender;

    thread_local! {
        static GATES: RefCell<[Option<Sender<()>>; 2]> = const { RefCell::new([None, None]) };
    }
    fn initial() -> App {
        let (s, article, discussion) = gated_summary_fixture();
        GATES.with(|gates| *gates.borrow_mut() = [Some(article), Some(discussion)]);
        s
    }
    fn reduced() -> App { let mut s = initial(); s.reduced = true; s }
    fn gated_poll(s: &mut App, now: f64, tx: &mut Tx) -> bool {
        let gate = GATES.with(|gates| {
            let mut gates = gates.borrow_mut();
            if now >= 0.7 && gates[1].is_some() { gates[1].take().map(|g| (g, false)) }
            else if now >= 0.2 && gates[0].is_some() { gates[0].take().map(|g| (g, true)) }
            else { None }
        });
        if let Some((gate, article)) = gate {
            gate.send(()).unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            let mut changed = false;
            loop {
                changed |= poll(s, now, tx);
                let story = s.story.as_ref().unwrap();
                let done = if article { matches!(story.article_summary, crate::story::ArticleSummaryState::Done(_)) }
                    else { matches!(story.summary, SummaryState::Done(_)) };
                if done { return changed; }
                assert!(std::time::Instant::now() < deadline, "gated result did not land");
                std::thread::yield_now();
            }
        }
        poll(s, now, tx)
    }
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/ux-motion/reading-arrivals");
    std::fs::create_dir_all(&out).unwrap();
    for (name, init, is_reduced) in [("normal", initial as fn() -> App, false), ("reduced", reduced, true)] {
        for theme in ["dark", "light"] {
            for (section, at) in [("article", "0.25"), ("article", "0.30"), ("discussion", "0.75"), ("discussion", "0.80")] {
                let stem = out.join(format!("{section}-{name}-{theme}-{}", at.replace('.', "")));
                let args = rune::tooling::Args::parse_from(vec!["reading-arrival".into(),
                    "--size".into(), "1180x760".into(), "--scale".into(), "1".into(),
                    "--theme".into(), theme.into(), "--at".into(), at.into(),
                    "--snap".into(), stem.with_extension("png").display().to_string(),
                    "--tree".into(), stem.with_extension("json").display().to_string(),
                    "--trace".into(), stem.with_extension("jsonl").display().to_string()]);
                let mut spec = AppSpec::new("Hacker News", (1180.0, 760.0), init, view);
                spec.poll = Some(gated_poll);
                rune::tooling::headless(rune::App::new(spec), args);
                let tree: serde_json::Value = serde_json::from_slice(&std::fs::read(stem.with_extension("json")).unwrap()).unwrap();
                let key = format!("{section}-summary-result");
                let node = tree["elements"].as_array().unwrap().iter().find(|e| e["key"] == key).unwrap();
                let opacity = node["opacity"].as_f64().unwrap();
                if is_reduced { assert_eq!(opacity, 1.0); }
                else { assert!(opacity > 0.4 && opacity < 1.0, "{section} {at}: {opacity}"); }
            }
        }
    }
}

#[test]
fn article_summary_arrives_before_discussion_and_survives_a_refresh_during_generation() {
    use std::sync::Mutex;
    use tokio::sync::oneshot;
    let (release, gate) = oneshot::channel();
    let gate = Mutex::new(Some(gate));
    let mut fake = crate::service::fake::FakeService::fixture();
    fake.summary = Arc::new(move |_| {
        let gate = gate.lock().unwrap().take().unwrap();
        Box::pin(async move { let _ = gate.await; Ok(crate::fixtures::summary()) })
    });
    let mut s = app(false, SavedView::in_memory());
    s.source = Arc::new(Source::Fake(fake));
    s.config.api_key = "fixture".into();
    s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
    poll_until(&mut s, |s| matches!(s.story.as_ref().unwrap().article_summary, crate::story::ArticleSummaryState::Done(_))
        && matches!(s.story.as_ref().unwrap().summary, SummaryState::Generating { .. }));
    let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
    let mut new_root = crate::fixtures::comments().remove(0);
    new_root.id = 987654;
    new_root.child_ids.clear();
    fake.comments.lock().unwrap().push(new_root);
    s.refresh_comments();
    poll_until(&mut s, |s| s.story.as_ref().unwrap().thread.node(987654).is_some());
    release.send(()).unwrap();
    poll_until(&mut s, |s| matches!(s.story.as_ref().unwrap().summary, SummaryState::Done(_)));
    let story = s.story.as_ref().unwrap();
    assert!(story.thread.node(987654).is_some());
    assert!(story.current_sources.iter().any(|source| source.id == 987654));
    assert!(!story.sources.iter().any(|source| source.id == 987654), "summary retains its original capture");
}

#[test]
fn reopening_reuses_pending_and_completed_summaries_across_stories() {
    use std::sync::{Mutex, atomic::{AtomicUsize, Ordering}};
    use tokio::sync::oneshot;
    let (article_release, article_gate) = oneshot::channel();
    let (discussion_release, discussion_gate) = oneshot::channel();
    let article_gate = Mutex::new(Some(article_gate));
    let discussion_gate = Mutex::new(Some(discussion_gate));
    let captures = Arc::new(AtomicUsize::new(0));
    let mut fake = crate::service::fake::FakeService::fixture();
    let captured = captures.clone();
    fake.article = Arc::new(move |_| {
        captured.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(("captured".into(), false)) })
    });
    fake.article_summary = Arc::new(move |id| {
        let gate = (id == 1).then(|| article_gate.lock().unwrap().take().unwrap());
        Box::pin(async move {
            if let Some(gate) = gate { gate.await.unwrap(); }
            Ok(hn_lib::reader::Article { text: format!("article {id}") })
        })
    });
    fake.summary = Arc::new(move |id| {
        let gate = (id == 1).then(|| discussion_gate.lock().unwrap().take().unwrap());
        Box::pin(async move {
            if let Some(gate) = gate { gate.await.unwrap(); }
            let mut summary = crate::fixtures::summary();
            summary.overview = format!("summary {id}");
            Ok(summary)
        })
    });
    let mut s = app(false, SavedView::in_memory());
    s.source = Arc::new(Source::Fake(fake));
    s.config.api_key = "fixture".into();
    let a = crate::fixtures::item(0, 1);
    let b = crate::fixtures::item(1, 2);
    s.open_story(a.clone());
    poll_until(&mut s, |s| {
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        *fake.article_summary_requests.lock().unwrap() == 1 && *fake.summary_requests.lock().unwrap() == 1
    });
    let dir = temp_dir("settings-during-paid-work");
    let mut settings = Settings::open(&s.config, Some(dir.join("summary.json")), false);
    settings.set_key("new-key"); settings.set_profile(hn_lib::reader::Profile::Thorough);
    s.settings = Some(settings); s.apply_settings(false);
    assert!(s.settings.is_none() && s.config.key() == "new-key");
    std::fs::remove_dir_all(dir).unwrap();
    s.close_story();
    s.open_story(a.clone());
    assert!(s.story.as_ref().unwrap().summary.is_generating());
    s.open_story(b.clone());
    poll_until(&mut s, |s| !s.bridge.is_story_pending());
    article_release.send(()).unwrap();
    discussion_release.send(()).unwrap();
    s.open_story(a.clone());
    poll_until(&mut s, |s| !s.bridge.is_story_pending());
    let story = s.story.as_ref().unwrap();
    assert!(matches!(&story.article_summary, crate::story::ArticleSummaryState::Done(text) if text == "article 1"));
    assert!(matches!(&story.summary, SummaryState::Done(summary) if summary.overview == "summary 1"));
    s.open_story(b);
    assert!(matches!(&s.story.as_ref().unwrap().summary, SummaryState::Done(summary) if summary.overview == "summary 2"));
    s.close_story();
    s.open_story(a);
    assert!(!s.bridge.is_story_pending(), "completed readings do not start another request");
    let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
    assert_eq!((*fake.article_summary_requests.lock().unwrap(), *fake.summary_requests.lock().unwrap()), (2, 2));
    assert_eq!(captures.load(Ordering::SeqCst), 2, "cached opens do not recapture articles");
}

#[test]
fn parked_capture_waits_for_reopen_and_respects_clear_then_restore() {
    use std::sync::{Mutex, mpsc};
    use tokio::sync::oneshot;
    for clear in [false, true] {
        let (capture_release, capture_gate) = oneshot::channel();
        let capture_gate = Mutex::new(Some(capture_gate));
        let (captured, captured_rx) = mpsc::channel();
        let (summary_release, summary_gate) = oneshot::channel();
        let summary_gate = Mutex::new(Some(summary_gate));
        let mut fake = crate::service::fake::FakeService::fixture();
        fake.article = Arc::new(move |_| {
            let gate = capture_gate.lock().unwrap().take().unwrap();
            let captured = captured.clone();
            Box::pin(async move { gate.await.unwrap(); captured.send(()).unwrap(); Ok(("captured".into(), false)) })
        });
        fake.summary = Arc::new(move |_| {
            let gate = summary_gate.lock().unwrap().take();
            Box::pin(async move { if let Some(gate) = gate { gate.await.unwrap(); } Ok(crate::fixtures::summary()) })
        });
        let mut s = app(false, SavedView::in_memory());
        s.source = Arc::new(Source::Fake(fake));
        s.config.api_key = "old".into();
        let item = crate::fixtures::item(0, 1);
        s.open_story(item.clone());
        poll_until(&mut s, |s| {
            let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
            *fake.summary_requests.lock().unwrap() == 1
        });
        s.close_story();
        if clear {
            let dir = temp_dir("parked-clear-restore");
            for key in ["", "new"] {
                let mut settings = Settings::open(&s.config, Some(dir.join("summary.json")), false);
                settings.set_key(key);
                s.settings = Some(settings);
                s.apply_settings(false);
                assert!(s.settings.is_none());
            }
            std::fs::remove_dir_all(dir).unwrap();
        }
        capture_release.send(()).unwrap();
        captured_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        summary_release.send(()).unwrap();
        for _ in 0..5 { poll(&mut s, 0.0, &mut Tx::default()); }
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        assert_eq!(*fake.article_summary_requests.lock().unwrap(), 0, "parked captures do not start a provider");
        s.open_story(item);
        poll_until(&mut s, |s| !s.bridge.is_story_pending());
        let story = s.story.as_ref().unwrap();
        assert!(matches!(story.article, crate::story::ArticleState::Ready { .. }));
        if clear {
            assert_eq!(story.article_summary, crate::story::ArticleSummaryState::Unconfigured);
            assert_eq!(story.summary, SummaryState::Unconfigured);
            let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
            assert_eq!((*fake.article_summary_requests.lock().unwrap(), *fake.summary_requests.lock().unwrap()), (0, 1));
            s.retry_summary();
            poll_until(&mut s, |s| !s.bridge.is_story_pending());
        } else {
            assert!(matches!(story.article_summary, crate::story::ArticleSummaryState::Done(_)));
            assert!(matches!(story.summary, SummaryState::Done(_)));
        }
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        assert_eq!((*fake.article_summary_requests.lock().unwrap(), *fake.summary_requests.lock().unwrap()),
            if clear { (1, 2) } else { (1, 1) });
    }
}

#[test]
fn clearing_a_parked_retry_preserves_both_previous_summaries() {
    use std::sync::Mutex;
    use tokio::sync::oneshot;
    let mut s = app(false, SavedView::in_memory());
    s.config.api_key = "old".into();
    let item = crate::fixtures::item(0, 1);
    s.open_story(item.clone());
    poll_until(&mut s, |s| !s.bridge.is_story_pending());
    let story = s.story.as_ref().unwrap();
    let article = story.article_summary.readable().unwrap().to_owned();
    let discussion = story.summary.readable().unwrap().clone();
    let sources: Vec<_> = story.sources.iter().map(|source| (source.id, source.text.clone())).collect();
    let (article_release, article_gate) = oneshot::channel();
    let (discussion_release, discussion_gate) = oneshot::channel();
    let article_gate = Mutex::new(Some(article_gate));
    let discussion_gate = Mutex::new(Some(discussion_gate));
    let Source::Fake(fake) = Arc::get_mut(&mut s.source).unwrap() else { unreachable!() };
    fake.article_summary = Arc::new(move |_| {
        let gate = article_gate.lock().unwrap().take().unwrap();
        Box::pin(async move { gate.await.unwrap(); Ok(hn_lib::reader::Article { text: "obsolete".into() }) })
    });
    fake.summary = Arc::new(move |_| {
        let gate = discussion_gate.lock().unwrap().take().unwrap();
        Box::pin(async move { gate.await.unwrap(); let mut summary = crate::fixtures::summary(); summary.overview = "obsolete".into(); Ok(summary) })
    });
    s.retry_summary();
    poll_until(&mut s, |s| {
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        *fake.article_summary_requests.lock().unwrap() == 2 && *fake.summary_requests.lock().unwrap() == 2
    });
    s.close_story();
    let dir = temp_dir("parked-clear-previous");
    for key in ["", "new"] {
        let mut settings = Settings::open(&s.config, Some(dir.join("summary.json")), false);
        settings.set_key(key);
        s.settings = Some(settings);
        s.apply_settings(false);
        assert!(s.settings.is_none());
    }
    std::fs::remove_dir_all(dir).unwrap();
    article_release.send(()).unwrap();
    discussion_release.send(()).unwrap();
    s.open_story(item);
    poll_until(&mut s, |s| !s.bridge.is_story_pending());
    let story = s.story.as_ref().unwrap();
    assert_eq!(story.article_summary.readable(), Some(article.as_str()));
    assert_eq!(story.summary.readable(), Some(&discussion));
    assert_eq!(story.sources.iter().map(|source| (source.id, source.text.clone())).collect::<Vec<_>>(), sources);
    let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
    assert_eq!((*fake.article_summary_requests.lock().unwrap(), *fake.summary_requests.lock().unwrap()), (2, 2));
}

#[test]
fn automatic_off_keeps_captures_without_provider_calls_until_regenerated() {
    for headless in [false, true] {
        let mut s = app(headless, SavedView::in_memory());
        s.config.api_key = "fixture".into();
        s.config.automatic = false;
        let item = crate::fixtures::item(0, 1);
        s.open_story(item.clone());
        poll_until(&mut s, |s| !s.bridge.is_story_pending());
        let story = s.story.as_ref().unwrap();
        assert!(matches!(story.article, crate::story::ArticleState::Ready { .. }));
        assert_eq!(story.discussion, Some(Ok(())));
        assert_eq!(story.article_summary, crate::story::ArticleSummaryState::Unconfigured);
        assert_eq!(story.summary, SummaryState::Unconfigured);
        s.refresh_comments();
        poll_until(&mut s, |s| !s.story.as_ref().unwrap().refreshing_comments);
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        assert_eq!((*fake.article_summary_requests.lock().unwrap(), *fake.summary_requests.lock().unwrap()), (0, 0));
        s.retry_summary();
        poll_until(&mut s, |s| !s.bridge.is_story_pending());
        let story = s.story.as_ref().unwrap();
        assert!(matches!(story.article_summary, crate::story::ArticleSummaryState::Done(_)));
        assert!(matches!(story.summary, SummaryState::Done(_)));
        s.close_story();
        s.open_story(item);
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        assert_eq!((*fake.article_summary_requests.lock().unwrap(), *fake.summary_requests.lock().unwrap()), (1, 1));
    }
}

#[test]
fn turning_automatic_off_skips_parked_capture_but_keeps_submitted_summary() {
    use std::sync::Mutex;
    use tokio::sync::oneshot;
    let (article_release, article_gate) = oneshot::channel();
    let (summary_release, summary_gate) = oneshot::channel();
    let article_gate = Mutex::new(Some(article_gate));
    let summary_gate = Mutex::new(Some(summary_gate));
    let mut fake = crate::service::fake::FakeService::fixture();
    fake.article = Arc::new(move |_| {
        let gate = article_gate.lock().unwrap().take().unwrap();
        Box::pin(async move { gate.await.unwrap(); Ok(("captured".into(), false)) })
    });
    fake.summary = Arc::new(move |_| {
        let gate = summary_gate.lock().unwrap().take().unwrap();
        Box::pin(async move { gate.await.unwrap(); Ok(crate::fixtures::summary()) })
    });
    let mut s = app(false, SavedView::in_memory());
    s.source = Arc::new(Source::Fake(fake));
    s.config.api_key = "fixture".into();
    let item = crate::fixtures::item(0, 1);
    s.open_story(item.clone());
    poll_until(&mut s, |s| {
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        *fake.summary_requests.lock().unwrap() == 1
    });
    s.close_story();
    let dir = temp_dir("automatic-off-parked");
    let mut settings = Settings::open(&s.config, Some(dir.join("summary.json")), false);
    settings.toggle_automatic();
    s.settings = Some(settings); s.apply_settings(false);
    assert!(s.settings.is_none() && !s.config.automatic);
    std::fs::remove_dir_all(dir).unwrap();
    article_release.send(()).unwrap();
    summary_release.send(()).unwrap();
    s.open_story(item);
    poll_until(&mut s, |s| !s.bridge.is_story_pending());
    let story = s.story.as_ref().unwrap();
    assert!(matches!(story.article, crate::story::ArticleState::Ready { .. }));
    assert_eq!(story.article_summary, crate::story::ArticleSummaryState::Unconfigured);
    assert!(matches!(story.summary, SummaryState::Done(_)), "submitted work remains visible");
    let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
    assert_eq!((*fake.article_summary_requests.lock().unwrap(), *fake.summary_requests.lock().unwrap()), (0, 1));
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0)); rt.frame(0.0);
    let elements = rt.elements();
    assert!(elements.iter().any(|e| e.key.as_deref() == Some("discussion-summary-heading")) && elements.iter().any(|e| e.key.as_deref() == Some("summary-text-0")));
    assert!(!elements.iter().any(|e| e.key.as_deref() == Some("article-summary-heading")));
    rt.update(|s, _| { let story = s.story.as_mut().unwrap(); story.summary = SummaryState::Unconfigured; story.article_summary = crate::story::ArticleSummaryState::Done("Article result".into()); }); rt.frame(0.0);
    let elements = rt.elements();
    assert!(elements.iter().any(|e| e.key.as_deref() == Some("article-summary-heading")) && !elements.iter().any(|e| e.key.as_deref() == Some("discussion-summary-heading")));
}

#[test]
fn manual_regeneration_while_capture_is_pending_works_with_automatic_off() {
    use std::sync::{Mutex, atomic::{AtomicUsize, Ordering}};
    use tokio::sync::oneshot;
    let (release, gate) = oneshot::channel();
    let gate = Mutex::new(Some(gate));
    let started = Arc::new(AtomicUsize::new(0));
    let captures = started.clone();
    let mut fake = crate::service::fake::FakeService::fixture();
    fake.article = Arc::new(move |_| {
        let gate = gate.lock().unwrap().take();
        captures.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { if let Some(gate) = gate { gate.await.unwrap(); } Ok(("captured".into(), false)) })
    });
    let mut s = app(false, SavedView::in_memory());
    s.source = Arc::new(Source::Fake(fake));
    s.config.api_key = "fixture".into();
    s.config.automatic = false;
    s.open_story(crate::fixtures::item(0, 1));
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while started.load(Ordering::SeqCst) == 0 {
        assert!(std::time::Instant::now() < deadline); std::thread::yield_now();
    }
    assert!(s.story.as_ref().unwrap().discussion.is_none());
    s.retry_summary();
    poll_until(&mut s, |s| !s.bridge.is_story_pending());
    assert!(matches!(s.story.as_ref().unwrap().article_summary, crate::story::ArticleSummaryState::Done(_)));
    assert!(matches!(s.story.as_ref().unwrap().summary, SummaryState::Done(_)));
    let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
    assert_eq!((*fake.article_summary_requests.lock().unwrap(), *fake.summary_requests.lock().unwrap()), (1, 1));
    release.send(()).unwrap();
}

#[test]
fn clearing_the_key_blocks_old_results_and_deferred_provider_calls() {
    use std::sync::Mutex;
    use tokio::sync::oneshot;
    for regenerate in [false, true] {
        let (article_release, article_gate) = oneshot::channel();
        let article_gate = Mutex::new(Some(article_gate));
        let (summary_release, summary_gate) = oneshot::channel();
        let summary_gate = Mutex::new(Some(summary_gate));
        let mut fake = crate::service::fake::FakeService::fixture();
        fake.article = Arc::new(move |_| {
            let gate = article_gate.lock().unwrap().take().unwrap();
            Box::pin(async move { let _ = gate.await; Ok(("captured".into(), false)) })
        });
        fake.summary = Arc::new(move |_| {
            let gate = summary_gate.lock().unwrap().take().unwrap();
            Box::pin(async move { let _ = gate.await; Ok(crate::fixtures::summary()) })
        });
        let mut s = app(false, SavedView::in_memory());
        s.source = Arc::new(Source::Fake(fake));
        s.config.api_key = "fixture".into();
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        poll_until(&mut s, |s| {
            let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
            *fake.summary_requests.lock().unwrap() == 1
        });
        let dir = temp_dir("clear-key-during-capture");
        let mut settings = Settings::open(&s.config, Some(dir.join("hn/summary.json")), false);
        settings.set_key("");
        s.settings = Some(settings);
        s.apply_settings(regenerate);
        assert!(s.settings.is_none(), "the actual Apply path must save and close");
        assert!(!s.configured());
        std::fs::remove_dir_all(dir).unwrap();
        article_release.send(()).unwrap();
        poll_until(&mut s, |s| matches!(s.story.as_ref().unwrap().article, crate::story::ArticleState::Ready { .. }));
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        assert_eq!(*fake.article_summary_requests.lock().unwrap(), 0);
        summary_release.send(()).unwrap();
        poll_until(&mut s, |s| !s.bridge.is_story_pending());
        let story = s.story.as_ref().unwrap();
        assert_eq!(story.summary, SummaryState::Unconfigured);
        assert_eq!(story.article_summary, crate::story::ArticleSummaryState::Unconfigured);
        assert!(!story.thread.is_empty());
    }
}

