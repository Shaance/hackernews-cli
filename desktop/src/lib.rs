//! `hn-desktop`'s modules, as a library so `tests/` can reach them.
//!
//! The binary is a two-line shell around [`app::spec`]; everything else lives here. The split
//! exists because the Saved store's real path is only testable from an integration test, where
//! `XDG_DATA_HOME` can be pointed at a temp directory without racing the unit tests.

pub mod app;
pub mod bookmarks;
pub mod bridge;
pub mod feed;
pub mod fixtures;
pub mod keys;
pub mod service;
