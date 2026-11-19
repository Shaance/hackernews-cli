//! HackerNews TUI Application
//!
//! An interactive terminal user interface for browsing HackerNews stories and comments.

use anyhow::Result;
use std::path::Path;
use tokio::sync::mpsc;

mod snapshot_capture;

use hn_lib::{
    app::App,
    bookmarks,
    hn_client::HackerNewsClientImpl,
    reader,
    snapshot::Snapshot,
    terminal_session::TerminalSession,
    theme::Theme,
    tui::{request_stories, run_app},
    HackerNewsCliServiceImpl,
};

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if matches!(args.first().map(String::as_str), Some("--version" | "-V")) {
        println!("hn {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("--capture-snapshot") {
        anyhow::ensure!(args.len() == 2, "Usage: hn --capture-snapshot PATH");
        return snapshot_capture::capture(Path::new(&args[1])).await;
    }
    if let [flag, path] = args.as_slice() {
        if flag == "--validate-snapshot" {
            Snapshot::load(Path::new(path))?;
            println!("Snapshot valid");
            return Ok(());
        }
        if flag == "--prepare-snapshot" {
            return snapshot_capture::prepare(Path::new(path)).await;
        }
    }
    let snapshot = match args.as_slice() {
        [] => None,
        [flag, path] if flag == "--snapshot" => {
            Some(std::sync::Arc::new(Snapshot::load(Path::new(path))?))
        }
        _ => anyhow::bail!("Usage: hn [--version | --snapshot PATH | --capture-snapshot PATH | --prepare-snapshot PATH | --validate-snapshot PATH]"),
    };
    let theme = Theme::detect();
    // crossterm drops colour codes whenever NO_COLOR is set; follow the resolved theme instead,
    // so an explicit HN_THEME wins and the monochrome theme stays colourless.
    crossterm::style::force_color_output(theme.uses_colour());
    let mut terminal = TerminalSession::new()?;

    // Create app state
    let mut app = App::with_theme(theme);
    app.snapshot = snapshot.clone();
    let mut bookmark_path = bookmarks::reload(&mut app);
    let service = snapshot
        .as_ref()
        .map(|data| {
            HackerNewsCliServiceImpl::new_with_client(HackerNewsClientImpl::with_snapshot(
                data.clone(),
            ))
        })
        .unwrap_or_default();

    // Create channel for async task communication
    let (tx, mut rx) = mpsc::unbounded_channel();

    // Load initial stories
    request_stories(&mut app, tx.clone(), service.clone(), false);

    let summary_root = if snapshot.is_none() {
        reader::saved::state_root().map(|root| root.join("summaries"))
    } else {
        None
    };
    let mut summaries = reader::Reader::new(theme, summary_root);
    if let Some(snapshot) = snapshot {
        summaries.load_snapshot(snapshot);
    }

    // Run the app
    let result = run_app(
        terminal.terminal_mut(),
        &mut app,
        tx,
        &mut rx,
        service,
        &mut bookmark_path,
        &mut summaries,
    )
    .await;
    terminal.restore();

    if let Err(err) = result {
        eprintln!("Error: {err}");
    }

    Ok(())
}
