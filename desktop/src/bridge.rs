//! The async bridge: one aggregate fill request at a time, driven off the UI thread.
//!
//! The desktop owns a tokio runtime. `AppSpec::setup` spawns the first fill; the
//! [`Latest`] worker closure drives the request to completion on that runtime, and `poll`
//! takes the answer. Cancellation is `Latest` generations only: a newer request supersedes
//! an older one, and a stale answer never lands.
//!
//! Headless runs and tests use [`Bridge::fill_now`], which runs the same request on the same
//! runtime but blocks, so a scene is deterministic and needs no frame budget.

use std::sync::Arc;

use anyhow::Result;
use rune::{Latest, Waker};
use tokio::runtime::{Handle, Runtime};

use crate::feed::{Fill, Filled};
use crate::service::Source;

/// One fill request: the window, plus the ranking when the caller already knows it.
///
/// Saved is the local store, not a network feed, so its ids come from the caller. Every other
/// tab asks the source for its snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub fill: Fill,
    pub ids: Option<Vec<i32>>,
}

impl Request {
    pub fn new(fill: Fill) -> Request {
        Request { fill, ids: None }
    }

    pub fn against(fill: Fill, ids: Vec<i32>) -> Request {
        Request {
            fill,
            ids: Some(ids),
        }
    }
}

pub struct Bridge {
    runtime: Runtime,
    handle: Handle,
    latest: Latest<Filled>,
}

impl Bridge {
    pub fn new() -> Result<Bridge> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("hn-desktop-io")
            .build()?;
        let handle = runtime.handle().clone();
        Ok(Bridge {
            runtime,
            handle,
            latest: Latest::default(),
        })
    }

    /// Attach the waker `AppSpec::setup` hands over, so a landed answer runs `poll`.
    pub fn set_waker(&self, waker: Waker) {
        self.latest.set_waker(waker);
    }

    /// Ask for `request`, superseding any earlier one. The work runs on the runtime from a
    /// `Latest` worker thread; only the newest request's answer lands.
    pub fn request(&self, source: Arc<Source>, request: Request) {
        let handle = self.handle.clone();
        self.latest.request(request, move |request| {
            handle.block_on(run(source.as_ref(), request))
        });
    }

    /// The same request, run to completion on this thread. Headless scenes and tests use it so
    /// the first frame already has its rows.
    pub fn fill_now(&self, source: &Source, request: Request) -> Filled {
        self.runtime.block_on(run(source, request))
    }

    /// The newest answer, once.
    pub fn take(&self) -> Option<Filled> {
        self.latest.take()
    }

    /// Whether a request is still in flight.
    pub fn is_pending(&self) -> bool {
        self.latest.is_pending()
    }

    /// Drop every request and any answer not yet taken.
    pub fn cancel(&self) {
        self.latest.cancel();
    }
}

/// The one request shape: take the ranking (or the caller's), then fetch the window against it.
async fn run(source: &Source, request: Request) -> Filled {
    let ids = match request.ids {
        Some(ids) => ids,
        None => source
            .ranked_ids(request.fill.tab)
            .await
            .unwrap_or_default(),
    };
    let items = source
        .fetch_pages_against(&ids, request.fill.from_rank, request.fill.count)
        .await;
    Filled {
        tab: request.fill.tab,
        ids,
        from_rank: request.fill.from_rank,
        items,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hn_lib::app::StoryType;
    use std::time::Duration;

    fn fill(tab: StoryType, from_rank: usize, count: usize) -> Fill {
        Fill {
            tab,
            from_rank,
            count,
        }
    }

    #[test]
    fn a_synchronous_fill_returns_the_window_in_rank_order() {
        let bridge = Bridge::new().unwrap();
        let source = Source::fake();
        let filled = bridge.fill_now(&source, Request::new(fill(StoryType::Top, 0, 11)));
        assert_eq!(filled.ids.len(), 30);
        assert_eq!(filled.items.len(), 11);
        assert_eq!(filled.items[0].as_ref().unwrap().id, 1);
        assert_eq!(filled.items[10].as_ref().unwrap().id, 11);
    }

    #[test]
    fn a_saved_fill_uses_the_callers_ids_and_never_the_source() {
        let bridge = Bridge::new().unwrap();
        let source = Source::fake();
        let filled = bridge.fill_now(
            &source,
            Request::against(fill(StoryType::Saved, 0, 4), vec![11, 6, 2, 1]),
        );
        assert_eq!(filled.ids, vec![11, 6, 2, 1]);
        assert_eq!(
            filled
                .items
                .iter()
                .flatten()
                .map(|item| item.id)
                .collect::<Vec<_>>(),
            vec![11, 6, 2, 1],
            "save-time order, not rank order"
        );
    }

    #[test]
    fn a_failed_item_keeps_its_slot() {
        let bridge = Bridge::new().unwrap();
        let source = Source::fake();
        if let Source::Fake(fake) = &source {
            fake.fail(3);
        }
        let filled = bridge.fill_now(&source, Request::new(fill(StoryType::Top, 0, 5)));
        assert_eq!(filled.items.len(), 5);
        assert!(filled.items[2].is_none());
    }

    #[test]
    fn a_newer_request_supersedes_an_older_one() {
        let bridge = Bridge::new().unwrap();
        let source = Arc::new(Source::fake());
        // A slow request for Top, then a fast one for New: only New's answer lands.
        if let Source::Fake(fake) = source.as_ref() {
            fake.fail(1);
        }
        bridge.request(
            source.clone(),
            Request::new(fill(StoryType::Top, 0, 11)),
        );
        bridge.request(source, Request::new(fill(StoryType::New, 0, 11)));
        let landed = wait_for(&bridge);
        assert_eq!(landed.tab, StoryType::New);
        assert_eq!(landed.items[0].as_ref().unwrap().id, 101);
        assert!(bridge.take().is_none(), "an answer lands once");
    }

    #[test]
    fn a_cancelled_request_never_lands() {
        let bridge = Bridge::new().unwrap();
        let source = Arc::new(Source::fake());
        bridge.request(source, Request::new(fill(StoryType::Top, 0, 11)));
        bridge.cancel();
        std::thread::sleep(Duration::from_millis(120));
        assert!(bridge.take().is_none());
        assert!(!bridge.is_pending());
    }

    /// Poll for a landed answer, the way `App::poll` would.
    fn wait_for(bridge: &Bridge) -> Filled {
        for _ in 0..600 {
            if let Some(filled) = bridge.take() {
                return filled;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("no answer landed");
    }
}
