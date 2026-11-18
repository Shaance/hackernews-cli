//! Feed requests, paging fills and cancellation on the async bridge.

use std::sync::Arc;

use super::Bridge;
use crate::feed::{FeedWindow, Fill, Filled};
use crate::service::Source;

/// One fill request: the window, plus the ranking to page it against.
///
/// `snapshot` is the feed's `id_snapshot`. A page turn or a resize pages against the ranking
/// the feed already has, so the ranks on screen stay the ranks that were shown; only a refresh
/// (or the first load of a tab) asks the source for a new one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub fill: Fill,
    pub snapshot: Option<Vec<i32>>,
}

impl Request {
    /// A request that pages against `snapshot`, taking a fresh one only when there is none.
    pub fn new(fill: Fill, snapshot: Option<Vec<i32>>) -> Request {
        Request { fill, snapshot }
    }
}

impl Bridge {
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

    /// Drop only the feed request when a held window settles synchronously.
    pub fn cancel_feed(&self) {
        self.latest.cancel();
    }

}

/// The one request shape: take the ranking (or the caller's), then fetch the window against it.
///
/// A ranking that cannot be fetched is an error the feed renders with a retry, not an empty
/// list: `unwrap_or_default` here would turn a network failure into "no stories".
async fn run(source: &Source, request: Request) -> Filled {
    let ids = match request.snapshot {
        Some(ids) => ids,
        None => match source.ranked_ids(request.fill.tab).await {
            Ok(ids) => ids,
            Err(error) => {
                return Filled {
                    tab: request.fill.tab,
                    from_rank: request.fill.from_rank,
                    outcome: Err(format!("{error:#}")),
                };
            }
        },
    };
    let items = source
        .fetch_pages_against(&ids, request.fill.from_rank, request.fill.count)
        .await
        .into_iter()
        .zip(ids.iter().skip(request.fill.from_rank))
        .map(|(item, &id)| item.ok_or(id))
        .collect();
    Filled {
        tab: request.fill.tab,
        from_rank: request.fill.from_rank,
        outcome: Ok(FeedWindow { ids, items }),
    }
}

