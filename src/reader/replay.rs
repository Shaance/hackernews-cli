//! Test-only composition of the real event loop with local fixture services.
use super::*;
use crate::{
    app::App,
    hn_client::{HackerNewsClientConfig, HackerNewsClientImpl},
    HackerNewsCliServiceImpl,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "PTY driver: scripts/verify-reader.sh"]
async fn terminal() {
    let base = std::env::var("HN_REPLAY_URL").expect("Use scripts/verify-reader.sh");
    let url = reqwest::Url::parse(&base).unwrap();
    assert_eq!(url.scheme(), "http");
    assert_eq!(url.host_str(), Some("127.0.0.1"));
    let service = HackerNewsCliServiceImpl::new_with_client(HackerNewsClientImpl::with_config(
        HackerNewsClientConfig {
            api_url: base.clone(),
            ..Default::default()
        },
    ));
    let theme = Theme {
        motion: false,
        ..Theme::TRUE_DARK
    };
    let mut reader = Reader {
        theme,
        config: Config {
            api_key: "replay-only".into(),
            ..Default::default()
        },
        network: network::Network {
            client: reqwest::Client::new(),
            endpoint: format!("{base}chat/completions"),
            anthropic_endpoint: format!("{base}messages"),
            response_log_dir: None,
            service: service.clone(),
        },
        ..Reader::default()
    };
    let mut app = App::with_theme(theme);
    let mut bookmark_path = crate::bookmarks::reload(&mut app);
    let (tx, mut rx) = mpsc::unbounded_channel();
    crate::tui::request_stories(&mut app, tx.clone(), service.clone(), false);
    let mut terminal = crate::terminal_session::TerminalSession::new().unwrap();
    let result = crate::tui::run_app(
        terminal.terminal_mut(),
        &mut app,
        tx,
        &mut rx,
        service,
        &mut bookmark_path,
        &mut reader,
    )
    .await;
    terminal.restore();
    result.unwrap();
}
