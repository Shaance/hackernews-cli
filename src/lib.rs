//! `hn`'s library half: the terminal app, the shared seams, and the reader.
//!
//! # Visibility of the terminal front end
//!
//! `tui` and `terminal_session` are `pub` because the binary is a *separate crate*: `src/main.rs`
//! is `hn`'s `[[bin]]` and reaches them as `hn_lib::tui::{request_stories, run_app}` and
//! `hn_lib::terminal_session::TerminalSession`. `pub(crate)` would not compile the binary, so
//! these two are the minimum `pub` the crate needs, not an invitation to bind to them: a
//! non-terminal front end drives the reader through [`reader`]'s documented boundary and never
//! touches the terminal front end.
//!
//! `reader_actions` is `pub(crate)`: only `tui` and the reader's own tests call it, so nothing
//! outside the crate can reach it.
pub mod app;
pub mod bookmarks;
pub mod comments;
pub mod event;
pub mod feed_state;
mod hn_cli_service;
pub mod hn_client;
pub mod reader;
pub(crate) mod reader_actions;
pub mod snapshot;
pub mod terminal_session;
pub mod theme;
mod time_utils;
pub mod tui;
pub mod ui;
pub mod url_open;

pub use hn_cli_service::{HNCLIItem, HackerNewsCliService, HackerNewsCliServiceImpl};

#[cfg(test)]
mod reading_tests;
