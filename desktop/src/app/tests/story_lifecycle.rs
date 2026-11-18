//! Story opening, native ownership and phase handoffs.

use super::super::*;
use std::time::Duration;
use rune::KeyEvent;
use rune::Tx;
use crate::story::SummaryState;
use super::super::input::on_key;
use super::super::poll::poll;
use super::super::window::view;
use super::support::*;

#[test]
fn feed_story_handoffs_remove_outgoing_paint_in_both_motion_modes() {
    for reduced in [false, true] {
        let mut s = app(true, saved_store());
        s.reduced = reduced;
        s.request(DEFAULT_GRID, Ranking::Fresh);
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.on_key = Some(on_key);
        rt.frame(0.0);
        for (key, incoming, outgoing, leading, old_leading) in [
            (rune::Key::Enter, "story-main", "feed-list", "story-toolbar", "feed-tabs"),
            (rune::Key::Escape, "feed-list", "story-main", "feed-tabs", "story-toolbar"),
        ] {
            rt.key(KeyEvent { key, mods: Default::default() });
            rt.frame(0.0);
            rt.frame(0.05);
            let elements = rt.elements();
            for wanted in [incoming, leading] {
                let node = elements.iter().find(|e| e.key.as_deref() == Some(wanted) && !e.exiting).unwrap();
                assert!(node.opacity > 0.0 && (reduced || node.opacity < 1.0),
                    "{wanted} reduced={reduced}: {}", node.opacity);
            }
            assert!(!elements.iter().any(|e| [outgoing, old_leading].contains(&e.key.as_deref().unwrap_or("")) && e.opacity > 0.001),
                "outgoing {outgoing} or {old_leading} still paints");
            rt.frame(0.5);
        }
    }
}

#[test]
fn ready_comments_are_not_blocked_by_two_superseded_stories() {
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::oneshot;

    let (article_one_tx, article_one_rx) = oneshot::channel::<()>();
    let (article_two_tx, article_two_rx) = oneshot::channel::<()>();
    let (summary_one_tx, summary_one_rx) = oneshot::channel::<()>();
    let (summary_two_tx, summary_two_rx) = oneshot::channel::<()>();
    let article_gates = Mutex::new(HashMap::from([(1, article_one_rx), (2, article_two_rx)]));
    let summary_gates = Mutex::new(HashMap::from([(1, summary_one_rx), (2, summary_two_rx)]));
    let articles_started = Arc::new(AtomicUsize::new(0));
    let summaries_started = Arc::new(AtomicUsize::new(0));
    let article_starts = articles_started.clone();
    let summary_starts = summaries_started.clone();
    let mut fake = crate::service::fake::FakeService::fixture();
    fake.article = Arc::new(move |id| {
        let gate = article_gates.lock().unwrap().remove(&id);
        if gate.is_some() { article_starts.fetch_or(1 << (id - 1), Ordering::SeqCst); }
        Box::pin(async move {
            if let Some(gate) = gate { let _ = gate.await; }
            Ok((crate::fixtures::ARTICLE.to_string(), false))
        })
    });
    fake.summary = Arc::new(move |id| {
        let gate = summary_gates.lock().unwrap().remove(&id);
        if gate.is_some() { summary_starts.fetch_or(1 << (id - 1), Ordering::SeqCst); }
        Box::pin(async move {
            if let Some(gate) = gate { let _ = gate.await; }
            Ok(crate::fixtures::summary())
        })
    });
    let mut s = app(false, SavedView::in_memory());
    s.source = Arc::new(Source::Fake(fake));
    s.config.api_key = "fixture".into();
    for id in [1, 2] {
        s.open_story(crate::fixtures::item((id - 1) as usize, id));
        let mask = (1 << id) - 1;
        poll_until(&mut s, |s| s.story.as_ref().unwrap().discussion == Some(Ok(()))
            && articles_started.load(Ordering::SeqCst) & mask == mask
            && summaries_started.load(Ordering::SeqCst) & mask == mask);
    }

    // Both old articles and both old summaries are held while story 3 starts.
    s.open_story(crate::fixtures::item(2, 3));
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while s.story.as_ref().unwrap().discussion.is_none() && std::time::Instant::now() < deadline {
        poll(&mut s, 0.0, &mut Tx::default());
        std::thread::sleep(Duration::from_millis(1));
    }
    let roots_arrived_while_old_work_was_held = s.story.as_ref().unwrap().discussion == Some(Ok(()));
    // Release one stale result and show that the third discussion itself succeeds.
    assert!(summary_one_tx.send(()).is_ok());
    poll_until(&mut s, |s| s.story.as_ref().unwrap().discussion == Some(Ok(())));
    assert_eq!(s.story.as_ref().unwrap().id(), 3);
    let _ = article_one_tx.send(());
    let _ = article_two_tx.send(());
    let _ = summary_two_tx.send(());
    s.bridge.cancel();
    assert!(roots_arrived_while_old_work_was_held,
        "ready comments waited for a superseded article or summary to release a Latest worker");
}

#[test]
fn story_phases_show_comments_while_work_is_pending_and_reject_stale_results() {
    use crate::story_io::SummaryError;
    use crate::story::ArticleState;
    use std::collections::VecDeque;
    use std::sync::{Mutex, mpsc};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::oneshot;

    let (article_tx, article_rx) = oneshot::channel();
    let article_rx = Arc::new(Mutex::new(Some(article_rx)));
    let (other_article_tx, other_article_rx) = oneshot::channel();
    let other_article_rx = Arc::new(Mutex::new(Some(other_article_rx)));
    let (failure_tx, failure_rx) = oneshot::channel();
    let (old_retry_tx, old_retry_rx) = oneshot::channel();
    let (old_story_tx, old_story_rx) = oneshot::channel();
    let summary_gates = Arc::new(Mutex::new(VecDeque::from([
        Some(failure_rx), Some(old_retry_rx), None, Some(old_story_rx), None,
    ])));
    let summary_started = Arc::new(AtomicUsize::new(0));
    let started = summary_started.clone();
    let (finished_tx, finished_rx) = mpsc::channel();
    let mut fake = crate::service::fake::FakeService::fixture();
    fake.article = Arc::new(move |story_id| {
        let gate = if story_id == 2 { other_article_rx.lock().unwrap().take() }
            else { article_rx.lock().unwrap().take() };
        Box::pin(async move {
            match gate {
                Some(gate) => gate.await.expect("article gate released"),
                None => Ok((crate::fixtures::ARTICLE.to_string(), false)),
            }
        })
    });
    fake.summary = Arc::new(move |story_id| {
        let gate = summary_gates.lock().unwrap().pop_front().expect("expected summary request");
        started.fetch_add(1, Ordering::SeqCst);
        let finished = finished_tx.clone();
        Box::pin(async move {
            match gate {
                Some(gate) => {
                    let result = gate.await.expect("summary gate released");
                    finished.send(story_id).unwrap();
                    result
                }
                None => {
                    let mut result = crate::fixtures::summary();
                    result.overview = format!("current summary {story_id}");
                    Ok(result)
                }
            }
        })
    });
    let mut s = app(false, SavedView::in_memory());
    s.source = Arc::new(Source::Fake(fake));
    s.config.api_key = "fixture".into();
    s.open_story(crate::fixtures::item(0, 1));
    poll_until(&mut s, |s| {
        s.story.as_ref().is_some_and(|story| story.discussion.as_ref().is_some_and(Result::is_ok))
            && summary_started.load(Ordering::SeqCst) == 1
    });
    let story = s.story.as_ref().unwrap();
    assert_eq!(story.article, ArticleState::Loading);
    assert!(matches!(story.summary, SummaryState::Generating { .. }));
    assert!(story.thread.node(901).is_some(), "roots arrive before article and summary");

    // Mount the actual inspector while both gated requests are still pending.
    s.live = Live::Failed { story_id: 1, error: "fixture".into() };
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.frame(0.4);
    assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("comment-901")));
    rt.update(|s, _| s.activate_comment(901));
    poll_until(&mut rt.state, |s| s.story.as_ref().unwrap().thread.children_of(901).len() == 2);

    assert!(article_tx.send(Ok(("extracted article".to_string(), false))).is_ok());
    poll_until(&mut rt.state, |s| matches!(s.story.as_ref().unwrap().article, ArticleState::Ready { .. }));
    assert!(matches!(rt.state.story.as_ref().unwrap().summary, SummaryState::Generating { .. }));
    assert!(failure_tx.send(Err(SummaryError::new("provider failed", false))).is_ok());
    poll_until(&mut rt.state, |s| s.story.as_ref().unwrap().summary.is_failed());
    let story = rt.state.story.as_ref().unwrap();
    assert!(matches!(&story.article, ArticleState::Ready { text, .. } if text == "extracted article"));
    assert_eq!(story.thread.children_of(901).len(), 2, "summary failure keeps loaded replies");

    // Two retries for the same story: the earlier provider finishes after the newer one.
    rt.state.retry_summary();
    poll_until(&mut rt.state, |_| summary_started.load(Ordering::SeqCst) == 2);
    rt.state.retry_summary();
    poll_until(&mut rt.state, |s| matches!(&s.story.as_ref().unwrap().summary,
        SummaryState::Done(summary) if summary.summary().overview == "current summary 1"));
    assert!(old_retry_tx.send(Err(SummaryError::new("obsolete failure", true))).is_ok());
    assert_eq!(finished_rx.recv_timeout(Duration::from_secs(3)).unwrap(), 1); // first failure
    assert_eq!(finished_rx.recv_timeout(Duration::from_secs(3)).unwrap(), 1); // old retry
    std::thread::sleep(Duration::from_millis(20));
    poll(&mut rt.state, 0.0, &mut Tx::default());
    assert!(matches!(&rt.state.story.as_ref().unwrap().summary,
        SummaryState::Done(summary) if summary.summary().overview == "current summary 1"));

    // A different story also wins over the older story's pending provider result.
    rt.state.retry_summary();
    poll_until(&mut rt.state, |_| summary_started.load(Ordering::SeqCst) == 4);
    rt.state.open_story(crate::fixtures::item(1, 2));
    poll_until(&mut rt.state, |s| matches!(&s.story.as_ref().unwrap().summary,
        SummaryState::Done(summary) if summary.summary().overview == "current summary 2"));
    assert!(matches!(rt.state.story.as_ref().unwrap().article, ArticleState::Loading));
    assert!(matches!(&rt.state.story.as_ref().unwrap().summary,
        SummaryState::Done(_)));
    assert!(other_article_tx.send(Ok(("second article".to_string(), false))).is_ok());
    poll_until(&mut rt.state, |s| matches!(&s.story.as_ref().unwrap().article,
        ArticleState::Ready { .. }));
    let mut stale = crate::fixtures::summary();
    stale.overview = "obsolete story".into();
    assert!(old_story_tx.send(Ok(stale)).is_ok());
    assert_eq!(finished_rx.recv_timeout(Duration::from_secs(3)).unwrap(), 1);
    std::thread::sleep(Duration::from_millis(20));
    poll(&mut rt.state, 0.0, &mut Tx::default());
    let story = rt.state.story.as_ref().unwrap();
    assert_eq!(story.id(), 2);
    assert!(matches!(&story.summary, SummaryState::Done(summary) if summary.summary().overview == "current summary 2"));
    assert!(story.thread.node(901).is_some());
}

#[test]
fn native_story_failure_and_layout_follow_story_identity() {
    let mut s = app(true, SavedView::in_memory());
    let a = crate::fixtures::item(0, 1);
    s.open_story(a.clone());
    s.live = Live::Failed { story_id: 1, error: "No host".into() };
    s.live_bounds = Some([10.0, 20.0, 400.0, 500.0]);
    s.close_story();
    assert!(matches!(s.live, Live::Failed { story_id: 1, .. }));
    s.open_story(a.clone());
    assert!(matches!(s.live, Live::Failed { story_id: 1, .. }));
    assert_eq!(s.live_bounds, None, "remounted region must supply fresh bounds");
    s.live_rect([30.0, 40.0, 500.0, 600.0]);
    assert_eq!(s.live_bounds, Some([30.0, 40.0, 500.0, 600.0]));
    s.open_story(a.clone());
    assert!(s.live_bounds.is_some(), "same mounted identity retains valid layout");
    let mut b = a; b.id = 2;
    s.open_story(b.clone());
    assert!(matches!(s.live, Live::Idle), "different IDs reset even with the same URL");
    assert_eq!(s.live_bounds, None);
    s.live = Live::Failed { story_id: 2, error: "No host".into() };
    s.open_story(b);
    assert!(matches!(s.live, Live::Failed { story_id: 2, .. }));
    s.open_story(crate::fixtures::item(2, 3));
    assert!(matches!(s.live, Live::Idle), "a new story can retry child construction");
}

#[test]
#[cfg(target_os = "macos")]
fn uncommitted_native_article_has_an_accessible_placeholder_and_current_layout_key() {
    let mut s = app(true, SavedView::in_memory());
    s.open_story(crate::fixtures::item(0, 1));
    s.headless = false;
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.frame(0.0);
    rt.frame(0.0);
    let elements = rt.elements();
    let region = elements.iter().find(|e| e.key.as_deref() == Some("article-live-1")).unwrap();
    assert!(region.rect[2] > 0.0 && region.rect[3] > 0.0);
    let status = elements.iter().find(|e| e.key.as_deref() == Some("article-loading")).unwrap();
    assert_eq!(status.role, "status");
    assert_eq!(rt.accessibility_tree().nodes.iter().filter(|(_, node)| node.is_busy()).count(), 1);
    assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("article-open-original") && e.label.as_deref() == Some("Open in browser")));
    assert!(rt.state.live_bounds.is_some(), "loading preserves the native layout reporter");
}

