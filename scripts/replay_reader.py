"""Drive the real terminal loop against recorded HN and model responses."""
import argparse
import html
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
from threading import Event
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
    # The rows just above the two-row footer always belong to the comment list.
    return text.splitlines()[-12:-2]


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
        match = re.search(r"COMMENTS  (\d+)", app.text())
        return int(match[1]) if match else 0
    def focus():
        return "".join(cell.data for row in app.screen.buffer.values() for cell in row.values()
                       if cell.fg in ("brown", "cdcd00") and cell.underscore and cell.bold and not cell.reverse)
    def passage():
        return [(row, int(m[1])) for row, line in enumerate(app.screen.display)
                if (m := re.search(r"Synthetic passage (\d+)", line))]
    try:
        app.wait(lambda t: "Nordwand Mono" in t and "123 points" in t)
        capture("01-feed")
        app.key("s4")
        app.wait(lambda t: "4 Saved" in t and "Nordwand Mono" in t)
        capture("02-saved")
        app.key("c")
        app.wait(lambda t: "Replay article control:" in t and count() == 40)
        assert len(fixture.calls) == 2
        assert any(c["status"] == "waiting" for c in fixture.calls)
        app.key("\x1b[6~")
        capture("03-partial-fast")
        fixture.gates[("fast", "discussion")].set()
        app.wait(lambda t: "✓ summary · Fast" in t)
        capture("04-fast")
        assert any(cell.fg != "default" for row in app.screen.buffer.values() for cell in row.values()), "Terminal colors were not captured"
        # PgDn before arrival froze the panel height: the discussion scrolls inside it.
        assert "J/K scroll" in app.text(), "Arrival after reading must not grow the panel"
        app.key("S")
        app.wait(lambda t: "S show" in t)
        app.key("S")
        app.wait(lambda t: "ARTICLE  Fast" in t and "J/K scroll" not in t)
        anchor = comments(app.text())
        app.key("S")
        app.wait(lambda t: "S show" in t)
        app.key("S")
        app.wait(lambda t: "ARTICLE  Fast" in t)
        assert comments(app.text()) == anchor, "Showing summary moved comments"
        assert len(fixture.calls) == 2, "Visibility must not generate"
        for n in range(1, 4):
            app.key("\t")
            assert focus() == f"[{n}]", "The exact citation token must be highlighted"
        app.key("\r")
        app.wait(lambda t: "source 3 of 8 · delta_p_delta_x · Fast" in t and "#50162345" in t)
        app.key("\t")
        app.wait(lambda t: "source 4 of 8 · kqr · Fast" in t and "#50162865" in t)
        app.key("\x1b[Z")
        app.wait(lambda t: "source 3 of 8 · delta_p_delta_x · Fast" in t and "#50162345" in t)
        capture("05-source")
        app.key("\x1b")
        app.wait(lambda t: "✓ summary · Fast" in t)
        assert comments(app.text()) == anchor, "Returning from source moved comments"
        assert focus() == "[3]" and len(fixture.calls) == 2
        app.key("M")
        app.wait(lambda t: "Summary settings" in t)
        capture("06-settings")
        app.key("\t \r")
        app.wait(lambda t: len(fixture.calls) == 4)
        assert "ARTICLE  Fast" in app.text(), "Upgrade removed retained output"
        capture("07-upgrade-pending")
        fixture.gates[("thorough", "article")].set()
        fixture.gates[("thorough", "discussion")].set()
        app.wait(lambda t: "! summary incomplete · M retry" in t and "HTTP 503" in t)
        capture("08-upgrade-failed")
        assert "ARTICLE  Thorough" in app.text() and "DISCUSSION  " in app.text()
        assert "DISCUSSION  Thorough unavailable · showing Fast" in app.text()
        app.key("Sr")
        app.wait(lambda t: "! summary incomplete · M retry" in t and "ARTICLE  " not in t and "DISCUSSION  " not in t)
        assert len(fixture.calls) == 4, "Refreshing comments must not retry a hidden summary"
        app.key("SM")
        app.wait(lambda t: "> Retry failed summary" in t and "Refresh sources & summary" in t)
        app.key("\r")
        app.wait(lambda t: "✓ summary · Thorough" in t and "DISCUSSION  Thorough" in t)
        capture("09-thorough")
        assert len(fixture.calls) == 5
        assert fixture.calls[-1]["section"] == "discussion", "Retry replaced successful article"
        app.key("\x1b")
        app.wait(lambda t: "4 Saved" in t)
        app.key("c")
        app.wait(lambda t: "✓ summary · Thorough" in t)
        assert len(fixture.calls) == 5, "Revisit must reuse summary"
        app.key("Sg")
        initial = count()
        app.key("\r")
        app.wait(lambda t: count() > initial)
        expanded = count()
        app.key("j]ucg")
        app.wait(lambda t: count() == initial)
        collapsed = count()
        capture("10-comments")
        # Synthetic controls are added only after all recorded summaries have completed.
        records = fixture.data["input"]["comments"]
        long_reply = {"id": 90000001, "parent": 50163214, "author": "synthetic_replay_control",
                      "text": "\n".join(f"Synthetic passage {i:03} · revision 1" for i in range(260))}
        records.insert(0, long_reply)
        app.key("rg")
        app.wait(lambda t: "3 replies" in t)
        app.key("\r")
        app.wait(lambda t: count() == 43)
        app.key("j\x1b[6~")
        before = passage()
        assert before and before[0][1] > 0, "Page Down must enter the long reply"
        capture("11-long-reply")
        root_path = "/v0/item/50141581.json"
        fixture.get_gates[root_path] = Event()
        start = len(fixture.gets)
        moved = next(c for c in records if c["id"] == 50164278)
        records.remove(moved)
        records.insert(0, moved)
        records.insert(0, {"id": 90000002, "parent": 50163214, "author": "synthetic_replay_control",
                           "text": "Synthetic inserted reply control above the reading passage."})
        long_reply["text"] = long_reply["text"].replace("revision 1", "revision 2")
        app.key("r")
        app.wait(lambda _: root_path in fixture.gets[start:])
        app.key("\x1b[6~")
        continued = passage()
        assert continued and continued[0][1] > before[0][1], "Reading must continue during refresh"
        capture("12-refresh-pending")
        fixture.get_gates[root_path].set()
        app.wait(lambda t: "revision 2" in t)
        assert passage() == continued, "Refresh moved the passage read while loading"
        assert not any(f"/v0/item/{ident}.json" in fixture.gets[start:] for ident in (50164961, 50164486)), "Refresh fetched an unopened branch"
        capture("13-refresh-retained")
        long_reply["text"] = long_reply["text"].replace("revision 2", "revision 3")
        fixture.fail_item = 90000001
        app.key("r")
        app.wait(lambda t: "Failed to load comments" in t)
        assert passage() == continued and "revision 2" in app.text(), "Failed child fetch discarded readable content"
        capture("14-refresh-failed")
        fixture.fail_item = None
        app.key("r")
        app.wait(lambda t: "revision 3" in t and "Failed to load comments" not in t)
        assert passage() == continued and len(fixture.calls) == 5
        capture("15-refresh-recovered")
        app.key("\x1b")
        app.wait(lambda t: "4 Saved" in t)
        app.key("1")
        app.wait(lambda t: "1 Top" in t and "Synthetic feed control 01" in t)
        for page in (2, 3):
            app.key("n")
            app.wait(lambda t: re.search(rf"Page {page}  $", t, re.M) and f"Synthetic feed control {(page - 1) * 20 + 1:02}" in t)
        app.key("j" * 19 + "s")
        app.wait(lambda t: "▎  60  Synthetic feed control 60 ★" in t)
        feed = app.text().splitlines()[2:-2]
        fixture.get_gates["/v0/newstories.json"] = Event()
        app.key("2")
        app.wait(lambda t: "1 Top" in t and re.search(r"Page 3  $", t, re.M) and re.search(r"fetching New p1  Page 3  $", t, re.M))
        capture("16-feed-pending")
        fixture.get_gates["/v0/newstories.json"].set()
        app.wait(lambda t: re.search(r"failed New p1  Page 3  $", t, re.M))
        app.key("4")
        app.wait(lambda t: "4 Saved" in t and "Synthetic feed control 60" in t)
        assert re.search(r"▎   2  .*Nordwand Mono", app.text()), "Saved insertion lost its selected story"
        app.key("k1")
        app.wait(lambda t: "1 Top" in t and re.search(r"Page 3  $", t, re.M) and "▎  60  Synthetic feed control 60 ★" in t)
        assert app.text().splitlines()[2:-2] == feed, "Feed return lost its scroll position"
        assert fixture.gets.count("/v0/topstories.json") == 3, "Feed return fetched again"
        capture("17-feed-restored")
        app.key("s4")
        app.wait(lambda t: "4 Saved" in t and "Synthetic feed control 60" not in t)
        assert re.search(r"▎   1  .*Nordwand Mono", app.text()), "Saved removal did not select the remaining story"
        capture("18-saved-current")
        app.quit()
        assert not fixture.errors, fixture.errors
        receipt = {"size": [width, height], "provider_calls": fixture.calls,
                   "reply_counts": [initial, expanded, collapsed], "captures": captured,
                   "passage_anchors": {"before_refresh": before[0], "during_and_after_refresh": continued[0]},
                   "hn_get_counts": {path: fixture.gets.count(path) for path in sorted(set(fixture.gets))},
                   "exit": "clean", "network": "Loopback fixtures; external HTTP blocked"}
        (output / "result.json").write_text(json.dumps(receipt, indent=2) + "\n")
        print(f"PASS {width}×{height}: recorded Fast, partial failure, independent Thorough, source cycling, explicit retry, refreshed passage, feed/Saved continuity, clean exit", flush=True)
    except BaseException:
        (output / "requests.json").write_text(json.dumps(fixture.calls, indent=2))
        capture("failure")
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
    (output / "index.html").write_text('<!doctype html><meta charset="utf-8"><title>Reader replay</title><h1>Reader replay</h1><p>Real terminal captures using recorded service responses. Synthetic article, feed and reply controls; real saved discussion summaries.</p><ul>' + "".join(links) + '</ul>')
    print(f"Open {output / 'index.html'}", flush=True)

if __name__ == "__main__":
    main()
