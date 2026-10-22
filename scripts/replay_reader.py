"""Drive the real terminal loop against recorded HN and model responses."""
import argparse
import html
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
from reader_fixture import ReaderFixture
from terminal_capture import ROOT, TUI


def executable():
    result = subprocess.run(["cargo", "test", "--bin", "hn", "--no-run", "--message-format=json"],
                            cwd=ROOT, text=True, stdout=subprocess.PIPE, check=True)
    for line in result.stdout.splitlines():
        item = json.loads(line)
        if item.get("reason") == "compiler-artifact" and item.get("executable") and item["target"]["name"] == "hn":
            return item["executable"]
    raise AssertionError("Cargo did not return the test executable")


def comments(text):
    # Summary bottom boundary; footer is deliberately excluded.
    return re.split(r"^╰[^\n]*\n", text, maxsplit=1, flags=re.M)[-1].splitlines()[:-2]


def run(binary, output, width, height):
    fixture = ReaderFixture()
    data = Path(tempfile.mkdtemp(prefix="hn-reader-replay-state-"))
    app = TUI(data, width, height, command=[binary, "reader::replay::terminal", "--exact", "--ignored", "--nocapture", "--test-threads=1"],
              env={"HN_REPLAY_URL": fixture.base, "HTTP_PROXY": "http://127.0.0.1:9", "HTTPS_PROXY": "http://127.0.0.1:9", "ALL_PROXY": "http://127.0.0.1:9", "NO_PROXY": "127.0.0.1,localhost"}, output=output)
    captured = []
    def capture(name):
        app.capture(name)
        captured.append(name)
    def count():
        match = re.search(r"DISCUSSION · (\d+) visible", app.text())
        return int(match[1]) if match else 0
    try:
        app.wait(lambda t: "Nordwand Mono" in t and "123 points" in t)
        capture("01-feed")
        app.key("s4")
        app.wait(lambda t: "Saved stories" in t and "Nordwand Mono" in t)
        capture("02-saved")
        app.key("c")
        app.wait(lambda t: "Replay article control:" in t and count() == 40)
        assert len(fixture.calls) == 2
        assert any(c["status"] == "waiting" for c in fixture.calls)
        app.key("\x1b[6~")
        capture("03-partial-fast")
        fixture.gates[("fast", "discussion")].set()
        app.wait(lambda t: "Summary ready" in t)
        capture("04-fast")
        assert any(cell.fg != "default" for row in app.screen.buffer.values() for cell in row.values()), "Terminal colors were not captured"
        anchor = comments(app.text())
        app.key("S")
        app.wait(lambda t: "S View" in t)
        app.key("S")
        app.wait(lambda t: "ARTICLE · Fast" in t)
        assert comments(app.text()) == anchor, "Showing summary moved comments"
        assert len(fixture.calls) == 2, "Visibility must not generate"
        app.key("\t\r")
        app.wait(lambda t: "CITED COMMENT" in t)
        capture("05-source")
        app.key("\x1b")
        app.wait(lambda t: "Summary ready" in t)
        assert comments(app.text()) == anchor, "Returning from source moved comments"
        app.key("M")
        app.wait(lambda t: "Summary settings" in t)
        capture("06-settings")
        app.key("\t \r")
        app.wait(lambda t: len(fixture.calls) == 4)
        assert "ARTICLE · Fast" in app.text(), "Upgrade removed retained output"
        capture("07-upgrade-pending")
        fixture.gates[("thorough", "article")].set()
        fixture.gates[("thorough", "discussion")].set()
        app.wait(lambda t: "Summary incomplete" in t and "HTTP 503" in t)
        capture("08-upgrade-failed")
        assert "ARTICLE · Thorough" in app.text() and "DISCUSSION ·" in app.text()
        assert "showing Fast" in app.text()
        app.key("r")
        app.wait(lambda t: "Summary ready" in t and "DISCUSSION · Thorough" in t)
        capture("09-thorough")
        assert len(fixture.calls) == 5
        assert fixture.calls[-1]["section"] == "discussion", "Retry replaced successful article"
        app.key("\x1bc")
        app.wait(lambda t: "Summary ready" in t)
        assert len(fixture.calls) == 5, "Revisit must reuse summary"
        app.key("Sg")
        initial = count()
        app.key("\r")
        app.wait(lambda t: count() > initial)
        expanded = count()
        app.key("j]ucg")
        app.wait(lambda t: count() == initial)
        capture("10-comments")
        app.key("\x1b")
        app.quit()
        assert not fixture.errors, fixture.errors
        receipt = {"size": [width, height], "provider_calls": fixture.calls,
                   "reply_counts": [initial, expanded, count()], "captures": captured,
                   "exit": "clean", "network": "Loopback fixtures; external HTTP blocked"}
        (output / "result.json").write_text(json.dumps(receipt, indent=2) + "\n")
        print(f"PASS {width}×{height}: recorded Fast, partial failure, independent Thorough, source/back, retained position, replies, clean exit", flush=True)
    except BaseException:
        capture("failure")
        (output / "requests.json").write_text(json.dumps(fixture.calls, indent=2))
        raise
    finally:
        app.abort()
        fixture.close()
    return captured


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--size", action="append", help="Terminal columns x rows; repeat for multiple captures")
    parser.add_argument("--output", type=Path, help="Artifact directory; defaults to a fresh temporary directory")
    args = parser.parse_args()
    sizes = []
    for value in args.size or ["240x80", "200x60"]:
        match = re.fullmatch(r"([1-9]\d*)x([1-9]\d*)", value)
        if not match:
            parser.error("Use --size COLUMNSxROWS")
        sizes.append(tuple(map(int, match.groups())))
    output = args.output or Path(tempfile.mkdtemp(prefix="hn-reader-verification-"))
    output.mkdir(parents=True, exist_ok=True)
    print(f"Captures: {output}", flush=True)
    binary = executable()
    links = []
    for width, height in sizes:
        folder = output / f"{width}x{height}"
        for name in run(binary, folder, width, height):
            links.append(f'<li><a href="{width}x{height}/{name}.html">{width}×{height} · {html.escape(name)}</a></li>')
    (output / "index.html").write_text('<!doctype html><meta charset="utf-8"><title>Reader replay</title><h1>Reader replay</h1><p>Real terminal captures using recorded service responses. Synthetic article control; real saved discussion summaries.</p><ul>' + "".join(links) + '</ul>')
    print(f"Open {output / 'index.html'}", flush=True)

if __name__ == "__main__":
    main()
