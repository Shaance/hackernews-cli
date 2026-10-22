//! Local experiment with terminal-browser v0.13.4; not a provider integration.
use anyhow::{anyhow, bail, Context, Result};
use hn_lib::{url_open::safe_story_url, HNCLIItem};
use serde_json::{json, Value};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
};

#[derive(Clone, Debug)]
pub struct Target {
    pub socket: String,
    pub tab: i64,
    story: i32,
    close_shortcut: bool,
}

#[cfg(unix)]
async fn control(socket: &str, mut request: Value) -> Result<Value> {
    tokio::time::timeout(Duration::from_secs(5), async {
        request["id"] = json!("hn-prototype");
        let mut stream = tokio::net::UnixStream::connect(socket).await?;
        stream.write_all(format!("{request}\n").as_bytes()).await?;
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).await?;
        let response: Value = serde_json::from_str(&line)?;
        if response["ok"] != true {
            bail!("{}", response["error"]);
        }
        Ok(response["data"].clone())
    })
    .await
    .context("Browser did not respond")?
}

pub async fn open(
    story: HNCLIItem,
    existing: Option<Target>,
    focus: bool,
) -> Result<(Option<Target>, String)> {
    let url = safe_story_url(&story.url, story.id);
    // Only touch the browser this CLI created. Never adopt an unrelated pane.
    if let Some(mut target) = existing {
        if std::path::Path::new(&target.socket).exists() {
            let targets = control(&target.socket, json!({"cmd":"targets"})).await?;
            let owns_tab = targets["tabs"]
                .as_array()
                .context("Missing browser tabs")?
                .iter()
                .any(|tab| tab["id"].as_i64() == Some(target.tab));
            let mut closed = true;
            if target.story != story.id || !owns_tab {
                let result = control(&target.socket, json!({"cmd":"open-tab", "url":url})).await?;
                let tab = result["openedTab"]
                    .as_i64()
                    .context("Missing article tab")?;
                if owns_tab {
                    closed = control(&target.socket, json!({"cmd":"close-tab", "tab":target.tab}))
                        .await
                        .is_ok();
                }
                target.tab = tab;
                target.story = story.id;
            }
            if !target.close_shortcut {
                target.close_shortcut = enable_close_shortcut(&target).await;
            }
            if focus {
                control(
                    &target.socket,
                    json!({"cmd":"activate-tab", "tab":target.tab}),
                )
                .await?;
                focus_browser(&target).await;
            }
            let notice = if !target.close_shortcut {
                "Article open · reload Ghostty, then o"
            } else if closed {
                "Article open · ⌘W closes browser"
            } else {
                "Article opened; previous tab could not be closed"
            };
            return Ok((Some(target), notice.into()));
        }
    }
    let ghostty = std::env::var("TERM_PROGRAM").is_ok_and(|v| v == "ghostty");
    let wide = crossterm::terminal::size().is_ok_and(|(width, _)| width >= 110);
    if !cfg!(target_os = "macos") || !ghostty || !wide {
        return external(&url);
    }
    let mut command = Command::new("terminal-browser");
    command
        .args([
            "open",
            &url,
            "--split",
            "right",
            "--size",
            "0.6",
            "--no-merge",
        ])
        .env("DO_NOT_TRACK", "1")
        .stdin(Stdio::inherit())
        .kill_on_drop(true);
    let output = match tokio::time::timeout(Duration::from_secs(30), command.output()).await {
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => return external(&url),
        Ok(result) => result?,
        Err(_) => bail!("Browser launch timed out; O opens externally"),
    };
    if !output.status.success() {
        bail!("Could not open browser pane; O opens externally");
    }
    let record: Value =
        serde_json::from_slice(&output.stdout).context("Browser launch response unreadable")?;
    let socket = record["socket"]
        .as_str()
        .context("Missing browser connection")?
        .to_owned();
    let targets = control(&socket, json!({"cmd":"targets"})).await?;
    let tab = targets["tabs"]
        .as_array()
        .and_then(|tabs| tabs.first())
        .and_then(|tab| tab["id"].as_i64())
        .ok_or_else(|| anyhow!("Missing article tab"))?;
    let mut target = Target {
        socket,
        tab,
        story: story.id,
        close_shortcut: false,
    };
    target.close_shortcut = enable_close_shortcut(&target).await;
    let notice = if target.close_shortcut {
        "Article open · ⌘W closes browser"
    } else {
        "Article open · reload Ghostty, then o"
    };
    Ok((Some(target), notice.into()))
}

fn external(url: &str) -> Result<(Option<Target>, String)> {
    open::that(url)?;
    Ok((None, "Opened in your browser".into()))
}

async fn focus_browser(target: &Target) {
    let Ok(where_) = control(&target.socket, json!({"cmd":"where"})).await else {
        return;
    };
    let Some(pane) = where_["pane"].as_str() else {
        return;
    };
    // The terminal ID is an argument, never interpolated into AppleScript code.
    let _ = Command::new("osascript")
        .args([
            "-e",
            "on run argv",
            "-e",
            "tell application \"Ghostty\" to focus (first terminal whose id is item 1 of argv)",
            "-e",
            "end run",
            pane,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
}

// The named table is defined in Ghostty config but activated only on our browser.
// The user reloads that config once; this integration never reloads other panes.
async fn enable_close_shortcut(target: &Target) -> bool {
    let Ok(where_) = control(&target.socket, json!({"cmd":"where"})).await else {
        return false;
    };
    let Some(pane) = where_["pane"].as_str() else {
        return false;
    };
    let result = Command::new("osascript")
        .args([
            "-e",
            "on run argv",
            "-e",
            "tell application \"Ghostty\"",
            "-e",
            "set browserTerminal to first terminal whose id is item 1 of argv",
            "-e",
            "return perform action \"activate_key_table:hn-article\" on browserTerminal",
            "-e",
            "end tell",
            "-e",
            "end run",
            pane,
        ])
        .output()
        .await;
    result.is_ok_and(|output| output.status.success() && output.stdout == b"true\n")
}

#[cfg(not(unix))]
async fn control(_socket: &str, _request: Value) -> Result<Value> {
    bail!("Article pane unsupported on this platform")
}

#[cfg(all(test, unix))]
#[path = "browser_tests.rs"]
mod tests;
