//! The Rune side of the spike: a window with a header row and a child-view region.
//!
//! The header is ordinary Rune UI. The region below it is an empty `el()` whose laid-out rect
//! is handed to the platform child host, which pins a native view to it. Nothing in Rune knows
//! a webview exists; the only contract is "here is a rect in window points, top-left origin".

use std::sync::OnceLock;
use std::time::Duration;

use rune::tok::{self, radius, type_};
use rune::{App, AppSpec, Cx, El, Key, KeyEvent, Tx, WindowHandle, col, el, row, text};

/// The page "u" loads. `AppSpec::init` is a plain `fn() -> S`, so the command line reaches the
/// state through this instead of a captured variable.
static REMOTE_URL: OnceLock<Option<String>> = OnceLock::new();

/// The marker the local page renders. The verification screenshots look for this string.
pub(crate) const MARKER: &str = "RUNE-WRY-CHILD-OK";

/// Synthetic local page: no network, no external assets. The clock and the click counter are
/// live JavaScript, so a screenshot showing a non-zero count proves a real web engine ran it
/// rather than a static image being blitted.
const LOCAL_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Rune wry child</title>
  <style>
    :root { color-scheme: light dark; font: 16px -apple-system, BlinkMacSystemFont, sans-serif; }
    body { margin: 0; padding: 28px 32px; background: #10261f; color: #eafff5; }
    .badge { display: inline-block; padding: 6px 10px; border-radius: 99px;
             background: #1f6f4f; color: #d6ffe9; font-size: 12px; font-weight: 700;
             letter-spacing: .06em; }
    h1 { margin: 18px 0 6px; font-size: 34px; letter-spacing: -.01em; }
    h1 code { font-family: ui-monospace, monospace; color: #7dffc0; }
    p { line-height: 1.5; color: #b7d9cb; margin: 6px 0; }
    .row { display: flex; gap: 18px; margin-top: 18px; flex-wrap: wrap; }
    .cell { padding: 10px 14px; border-radius: 9px; background: #17352b;
            border: 1px solid #24503f; min-width: 150px; }
    .cell b { display: block; font-size: 11px; letter-spacing: .08em; color: #6f9c8a;
              text-transform: uppercase; margin-bottom: 4px; }
    .cell span { font: 15px ui-monospace, monospace; color: #eafff5; }
    textarea { box-sizing: border-box; display: block; width: 100%; margin-top: 20px;
               min-height: 62px; padding: 10px 12px; border-radius: 9px;
               border: 1px solid #2c5f4b; background: #0c1f19; color: #eafff5;
               font: 14px ui-monospace, monospace; resize: vertical; }
    textarea:focus { outline: 3px solid #35d08a; outline-offset: 2px; }
  </style>
</head>
<body>
  <span class="badge">WKWebView CHILD OF A RUNE WINDOW</span>
  <h1><code>RUNE-WRY-CHILD-OK</code></h1>
  <p>This page is a wry child view pinned to the Rune element below the header.
     Resize the window and it follows; the numbers below are live JavaScript.</p>
  <div class="row">
    <div class="cell"><b>viewport</b><span id="size">-</span></div>
    <div class="cell"><b>clock</b><span id="clock">-</span></div>
    <div class="cell"><b>clicks</b><span id="clicks">0</span></div>
  </div>
  <textarea id="type" spellcheck="false"
    placeholder="Click here and type: keyboard focus and clipboard land in the webview"></textarea>
  <script>
    const size = document.getElementById('size');
    const clock = document.getElementById('clock');
    const clicks = document.getElementById('clicks');
    let n = 0;
    const paint = () => { size.textContent = innerWidth + ' x ' + innerHeight; };
    paint();
    addEventListener('resize', paint);
    setInterval(() => { clock.textContent = new Date().toLocaleTimeString(); }, 250);
    addEventListener('pointerdown', () => { clicks.textContent = ++n; });
  </script>
</body>
</html>"#;

/// What the child region currently holds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Child {
    /// No window handle yet, or no laid-out rect yet.
    Pending,
    /// The local marker page.
    Local,
    /// A remote page, loaded by "u".
    Remote,
    /// Creation failed; `State::error` says why.
    Failed,
}

struct State {
    native_window: Option<WindowHandle>,
    child_bounds: Option<[f32; 4]>,
    child: Child,
    error: Option<String>,
    /// The page "u" loads, from the command line.
    remote_url: Option<String>,
    /// How many times "w" hid or showed the child. The header prints it, so one screenshot
    /// proves the Rune UI and the child view are both live and independent.
    toggles: u32,
    visible: bool,
    started: bool,
}

pub(crate) fn run(argv: Vec<String>, remote_url: Option<String>) {
    let _ = REMOTE_URL.set(remote_url);
    let script = scripted(&argv);
    let on_top = argv.iter().any(|a| a == "--spike-on-top");
    let spec = AppSpec {
        min_size: Some((640.0, 480.0)),
        poll: Some(poll),
        on_key: Some(on_key),
        ..AppSpec::new("Rune wry child spike", (960.0, 680.0), State::default, view)
    };
    let mut app = App::new(spec).args(argv).standard_menus().on_close(close);
    if on_top {
        app = app.window_options(rune::WindowOptions { always_on_top: true, ..Default::default() });
    }
    let app_handle = app.handle();
    if !script.is_empty() {
        let handle = app_handle.clone();
        std::thread::spawn(move || {
            let start = std::time::Instant::now();
            for (at, action) in script {
                let elapsed = start.elapsed().as_secs_f64();
                if at > elapsed {
                    std::thread::sleep(Duration::from_secs_f64(at - elapsed));
                }
                eprintln!("SPIKE at={at:.1} action={action:?}");
                match action {
                    Action::Focus => handle.focus(),
                    Action::Resize(w, h) => handle.update(move |_, tx| tx.resize_window_instant((w, h))),
                    Action::Toggle => handle.update(move |state, _| toggle(state)),
                    Action::Remote => handle.update(move |state, _| {
                        load_remote(state);
                    }),
                }
            }
        });
    }
    app.on_window(move |window| {
        // A recreated window invalidates the old child: drop it and let `poll` build a new one.
        child::close();
        let native_window = window.clone();
        app_handle.update(move |state, _| {
            state.native_window = Some(native_window);
            state.started = false;
            state.child = Child::Pending;
        });
    })
    .try_run()
    .unwrap_or_else(|error| panic!("{error}"));
}

/// One scripted action, for GUI verification.
#[derive(Clone, Copy, Debug)]
enum Action {
    Resize(f32, f32),
    Toggle,
    Focus,
    Remote,
}

/// `--spike-at SECONDS:ACTION`, repeatable, where ACTION is `resize:WxH`, `toggle`, `focus`,
/// or `remote` (the same thing "u" does).
///
/// This machine denies osascript assistive access, so a shell script cannot resize the window
/// or press a key from outside. These run through `AppHandle::update` on the UI thread and take
/// the same paths a user's drag and a "w" press take: a real winit resize, a real relayout, a
/// real `on_layout`, and the same `toggle` the key and the button call.
fn scripted(argv: &[String]) -> Vec<(f64, Action)> {
    let mut out = Vec::new();
    for pair in argv.windows(2) {
        if pair[0] != "--spike-at" {
            continue;
        }
        let Some((at, action)) = pair[1].split_once(':') else {
            continue;
        };
        let Ok(at) = at.parse::<f64>() else {
            continue;
        };
        // `resize:WxH` carries an argument; the rest are bare words. Splitting on ':' first and
        // matching only `Some` would silently drop every bare action, so the two shapes are
        // matched separately.
        let action = match action.split_once(':') {
            Some(("resize", size)) => match size.split_once('x') {
                Some((w, h)) => match (w.parse::<f32>(), h.parse::<f32>()) {
                    (Ok(w), Ok(h)) => Action::Resize(w, h),
                    _ => continue,
                },
                None => continue,
            },
            Some(_) => continue,
            None => match action {
                "toggle" => Action::Toggle,
                "focus" => Action::Focus,
                "remote" => Action::Remote,
                _ => continue,
            },
        };
        out.push((at, action));
    }
    out
}

impl Default for State {
    fn default() -> Self {
        State {
            native_window: None,
            child_bounds: None,
            child: Child::Pending,
            error: None,
            remote_url: REMOTE_URL.get().cloned().flatten(),
            toggles: 0,
            visible: true,
            started: false,
        }
    }
}

/// Create the child once both a window handle and a laid-out rect exist. Runs every frame;
/// the work happens on the first frame where both are known.
fn poll(state: &mut State, _: f64, _: &mut Tx) -> bool {
    if state.started || state.error.is_some() {
        return false;
    }
    let (Some(window), Some(rect)) = (state.native_window.clone(), state.child_bounds) else {
        return false;
    };
    match child::create(&window, rect, LOCAL_HTML) {
        Ok(()) => {
            state.started = true;
            state.child = Child::Local;
        }
        Err(error) => {
            state.child = Child::Failed;
            state.error = Some(error);
        }
    }
    true
}

fn close(_: &mut State, _: &mut Tx) {
    child::close();
}

fn on_key(state: &mut State, event: &KeyEvent, _: &mut Tx) -> bool {
    match &event.key {
        Key::Text(t) if t.as_str() == "w" => {
            toggle(state);
            true
        }
        Key::Text(t) if t.as_str() == "u" => load_remote(state),
        _ => false,
    }
}

fn toggle(state: &mut State) {
    if !state.started {
        return;
    }
    state.visible = !state.visible;
    state.toggles += 1;
    child::set_visible(state.visible);
}

/// "u": load the remote page. Without a URL on the command line there is nothing to load, so
/// the header says so instead of silently doing nothing.
fn load_remote(state: &mut State) -> bool {
    let Some(url) = state.remote_url.clone() else {
        eprintln!("SPIKE load_remote: no url on the command line");
        return false;
    };
    if !state.started {
        eprintln!("SPIKE load_remote: child not started yet");
        return false;
    }
    eprintln!("SPIKE load_remote url={url}");
    match child::load_url(&url) {
        Ok(()) => {
            eprintln!("SPIKE load_remote ok");
            state.child = Child::Remote;
            state.visible = true;
            child::set_visible(true);
        }
        Err(error) => {
            eprintln!("SPIKE load_remote err={error}");
            state.error = Some(error);
        }
    }
    true
}

fn view(state: &State, cx: &mut Cx<State>) -> El {
    let dark = cx.system_dark();
    let theme = tok::theme(dark);
    let accent = tok::accent(0, dark);

    let status = match (&state.child, &state.error) {
        (Child::Failed, Some(error)) => format!("child failed: {error}"),
        (Child::Failed, None) => "child failed".to_owned(),
        (Child::Local, _) => format!("local page · {MARKER}"),
        (Child::Remote, _) => format!(
            "remote page · {}",
            state.remote_url.as_deref().unwrap_or("(none)")
        ),
        (Child::Pending, _) => "waiting for a window and a laid-out rect".to_owned(),
    };
    let bounds = state.child_bounds.map_or_else(
        || "no rect yet".to_owned(),
        |[x, y, w, h]| format!("child rect {x:.0},{y:.0} {w:.0}x{h:.0}"),
    );

    let header = row()
        .key("header")
        .w_full()
        .h(64.0)
        .pl(cx.titlebar_inset() + 22.0)
        .pr(20.0)
        .gap(14.0)
        .center_y()
        .fill(theme.card)
        .window_drag()
        .child(
            col()
                .gap(2.0)
                .child(
                    text("Rune wry child spike")
                        .font(type_::TITLE)
                        .color(theme.text),
                )
                .child(
                    text(format!("{status} · {bounds}"))
                        .font(type_::LABEL)
                        .color(theme.text_secondary),
                ),
        )
        .child(el().grow(1.0))
        .child(
            text(format!("w toggles · {} · u loads remote", state.toggles))
                .font(type_::LABEL)
                .color(theme.text_secondary),
        )
        .child(toggle_button(state, theme, accent));

    // `grow(1.0)` with a zero minimum, not `h_full()`: the child area is the space the header
    // leaves, so its rect never runs past the window's bottom edge.
    let mut area = el()
        .key("child-area")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .min_w(0.0)
        .fill(theme.bg)
        .on_layout(|state: &mut State, rect| {
            state.child_bounds = Some(rect);
            if state.started {
                child::set_bounds(rect);
            }
        });
    if let Some(error) = &state.error {
        area = area.child(
            text(format!("child view could not be created: {error}"))
                .key("child-error")
                .color(tok::accent(2, dark)),
        );
    }

    col()
        .key("window")
        .role("window")
        .w_full()
        .h_full()
        .fill(theme.bg)
        .child(header)
        .child(area)
}

/// A clickable "w": the same action as the key, so the toggle can be driven without the
/// keyboard while the webview holds first responder.
fn toggle_button(state: &State, theme: tok::Theme, accent: rune::Color) -> El {
    let label = if state.visible { "w  hide" } else { "w  show" };
    row()
        .key("toggle-w")
        .role("button")
        .label(label)
        .gap(6.0)
        .center_y()
        .h(24.0)
        .px(10.0)
        .radius(radius::SM)
        .fill(theme.control_off)
        .on_press(|state: &mut State| toggle(state))
        .child(text(label).font(type_::LABEL).color(accent))
}

#[cfg(target_os = "macos")]
use crate::webview as child;

/// Non-macOS: the Rune app still builds and renders headless; there is no child host.
#[cfg(not(target_os = "macos"))]
mod child {
    use rune::WindowHandle;

    pub(super) fn create(
        _window: &WindowHandle,
        _bounds: [f32; 4],
        _html: &str,
    ) -> Result<(), String> {
        Err("no child-view host on this platform".to_owned())
    }
    pub(super) fn set_bounds(_bounds: [f32; 4]) {}
    pub(super) fn set_visible(_visible: bool) {}
    pub(super) fn load_url(_url: &str) -> Result<(), String> {
        Err("no child-view host on this platform".to_owned())
    }
    pub(super) fn close() {}
}

#[cfg(test)]
mod tests {
    use super::{LOCAL_HTML, MARKER};

    #[test]
    fn local_page_is_synthetic_and_carries_the_marker() {
        assert!(LOCAL_HTML.contains(MARKER));
        assert!(!LOCAL_HTML.contains("http://"));
        assert!(!LOCAL_HTML.contains("https://"));
        assert!(!LOCAL_HTML.contains("src="));
        assert!(!LOCAL_HTML.contains("href="));
    }
}
