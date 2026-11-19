"""Drive the real terminal loop against recorded HN and model responses."""
import argparse
from collections import Counter
import fcntl
import html
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import tempfile
import termios
from threading import Event
from reader_fixture import ReaderFixture
from terminal_capture import ROOT, TUI


def executable():
    result = subprocess.run(["cargo", "test", "--locked", "--lib", "--no-run", "--message-format=json"],
                            cwd=ROOT, text=True, stdout=subprocess.PIPE, check=True)
    for line in result.stdout.splitlines():
        item = json.loads(line)
        if item.get("reason") == "compiler-artifact" and item.get("executable") and item["target"]["name"] == "hn_lib":
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
                       if cell.fg == "f3b77b" and cell.underscore and cell.bold and not cell.reverse)
    def passage():
        return [(row, int(m[1])) for row, line in enumerate(app.screen.display)
                if (m := re.search(r"Synthetic passage (\d+)", line))]

    def selected(author, depth):
        header = f"▎ {'│ ' * depth}{author}  "
        app.wait(lambda text: any(line.startswith(header) for line in text.splitlines()))

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
        app.key("\x1b[6~")
        assert comments(app.text()) != anchor, "Page Down must move the discussion before opening a source"
        for n in range(1, 4):
            app.key("\t")
            assert focus() == f"[{n}]", "The exact citation token must be highlighted"
        anchor = comments(app.text())
        before_source = app.text().splitlines()
        # At this point every discussion row is top-level, so only the summary has a rail.
        first_comment_row = max(i for i, row in enumerate(before_source) if row.startswith(("  │ ", "  ┃ "))) + 2
        passage_rows = slice(first_comment_row, first_comment_row + 3)
        source_anchor = before_source[passage_rows]
        assert any(line.strip() for line in source_anchor), "Expected discussion text under the summary"
        capture("04a-before-source")
        records = fixture.data["input"]["comments"]
        records.extend([
            {"id": 90000004, "parent": 50162980, "author": "source_nested_control",
             "text": "Nested reply loaded from the source view."},
            {"id": 90000005, "parent": 50164113, "author": "detached_reply_control",
             "text": "Reply to a citation in an unopened branch."},
        ])
        absent_path = "/v0/item/50164113.json"
        detached_reply_path = "/v0/item/90000005.json"
        start = len(fixture.gets)
        fixture.fail_item = 50164113
        app.key("\t\t\r")
        source = app.wait(lambda t: "source 5 of 8 · chirayuk · Fast" in t and "Cited comment unavailable · Enter retry" in t)
        assert "The hyphen is as wide as the em dash" in source
        assert "PARENT CONTEXT  kqr · #50162865" in source and "I have been going back and forth between Luxi Mono" in source
        capture("05a-source-parent")
        fixture.fail_item = None
        app.key("\r")
        app.wait(lambda t: "detached_reply_control" in t and "Enter retry" not in t)
        assert Counter(fixture.gets[start:]) == Counter({absent_path: 2, detached_reply_path: 1})
        capture("05b-source-retried")
        app.key("h\x1b[Z\x1b[Z")
        assert focus() == "[3]" and comments(app.text()) == anchor

        direct_path = "/v0/item/50162980.json"
        nested_source_path = "/v0/item/90000004.json"
        fixture.get_gates[direct_path] = Event()
        start = len(fixture.gets)
        app.key("\r")
        source = app.wait(lambda t: "source 3 of 8 · delta_p_delta_x · Fast" in t and
                          direct_path in fixture.gets[start:] and "loading replies" in t)
        assert "#50162345" in source and "The overall shape of the glyphs" in source
        assert "▎ Replies to cited comment" in source and "JdeBP  " not in source
        capture("05c-source-loading")
        fixture.get_gates.pop(direct_path).set()
        app.wait(lambda t: "JdeBP  " in t)
        app.key("j")
        selected("JdeBP", 1)
        app.key("k")
        assert "▎ Replies to cited comment" in app.text()
        app.key("jo")
        app.wait(lambda t: "source_nested_control" in t)
        app.key("j")
        selected("source_nested_control", 2)
        capture("05d-source-nested")
        app.key("c")
        assert "source_nested_control  " not in app.text()
        app.key("\t")
        app.wait(lambda t: "source 4 of 8 · kqr · Fast" in t and "detached_reply_control" in t)
        app.key("\x1b[Z")
        source = app.wait(lambda t: "source 3 of 8 · delta_p_delta_x · Fast" in t and "source_nested_control" in t)
        assert "▎ Replies to cited comment" in source and "#50162345" in source
        source_requests = Counter(fixture.gets[start:])
        assert source_requests == Counter({direct_path: 1, nested_source_path: 1})
        app.key("?")
        app.wait(lambda t: "CITATION SOURCE" in t and "Backspace" in t)
        app.key("?")
        capture("05-source")
        app.key("h")
        app.wait(lambda t: "✓ summary · Fast" in t)
        # A new branch can push lower rows down while the passage at the top stays put.
        assert app.text().splitlines()[passage_rows] == source_anchor, "Returning from source moved the reading passage"
        capture("05e-source-return")
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
        app.wait(lambda t: "▎ Retry failed summary" in t and "Refresh sources & summary" in t)
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
        # A synthetic grandchild exercises a deeper branch without changing the summary inputs.
        records = fixture.data["input"]["comments"]
        records.append({"id": 90000003, "parent": 50166096, "author": "nested_replay_control",
                        "text": "Synthetic grandchild for comment navigation."})
        reply_paths = [f"/v0/item/{ident}.json" for ident in (50166096, 50163340)]
        nested_path = "/v0/item/90000003.json"
        app.key("Sg")
        start = len(fixture.gets)
        app.key("]" * 11)
        selected("delta_p_delta_x", 0)
        app.key("jj")
        selected("source_nested_control", 2)
        assert not fixture.gets[start:], "The discussion requested replies already loaded from a source"
        capture("09a-source-replies-shared")
        app.key("g")
        selected("tywr", 0)
        initial = count()
        fixture.get_gates[reply_paths[0]] = Event()
        start = len(fixture.gets)
        app.key("\r")
        app.wait(lambda t: reply_paths[0] in fixture.gets[start:] and "loading replies" in t)
        assert count() == initial and "katspaugh  " not in app.text(), "A gated branch must show loading before its replies"
        capture("10a-replies-loading")
        fixture.get_gates.pop(reply_paths[0]).set()
        app.wait(lambda t: count() == initial + 2)
        expanded = count()
        rows = app.text().splitlines()
        children = [line.split()[1] for line in rows if line.startswith(("  │ katspaugh  ", "  │ bayindirh  "))]
        assert children == ["katspaugh", "bayindirh"], f"Expected both replies in HN order at depth 1, got {children}"
        capture("10b-replies-expanded")
        for key, author, depth in (("j", "katspaugh", 1), ("]", "bayindirh", 1),
                                   ("[", "katspaugh", 1), ("u", "tywr", 0)):
            app.key(key)
            selected(author, depth)
        # Exercise a late response before the first successful load of this branch.
        fixture.get_gates[nested_path] = Event()
        start = len(fixture.gets)
        app.key("j\r")
        app.wait(lambda t: nested_path in fixture.gets[start:] and "loading replies" in t)
        capture("10f-late-loading")
        app.key("c")
        selected("katspaugh", 1)
        folded = app.wait(lambda t: "loading replies" not in t and count() == expanded)
        fixture.get_gates[nested_path].set()
        app.wait(lambda _: nested_path in fixture.gated_responses)
        assert app.pump(0.4) == folded, "Late reply changed the folded view after the server sent it"
        late = Counter(fixture.gets[start:])
        assert late == Counter({nested_path: 1}), f"Expected one request for the collapsed load, got {dict(late)}"
        capture("10g-late-ignored")
        app.key("\r")
        app.wait(lambda t: count() == expanded + 1)
        app.key("j")
        selected("nested_replay_control", 2)
        capture("10c-deeper-reply")
        app.key("c")
        selected("katspaugh", 1)
        assert count() == expanded and "nested_replay_control" not in app.text(), "Collapse from descendant must hide the grandchild"
        capture("10d-deeper-folded")
        app.key("\r")
        app.wait(lambda t: count() == expanded + 1)
        app.key("uc")
        app.wait(lambda t: count() == initial)
        selected("tywr", 0)
        assert all(author not in app.text() for author in ("katspaugh  ", "bayindirh  ")), "Folding the root must hide both replies"
        collapsed = count()
        capture("10-comments")
        start = len(fixture.gets)
        app.key("\r")
        app.wait(lambda t: count() == expanded + 1 and "nested_replay_control" in t)
        app.pump(0.4)
        reopen = Counter(fixture.gets[start:])
        assert not reopen, f"Reopening the folded root and its nested expansion made requests: {dict(reopen)}"
        capture("10e-reopened")
        app.key("jcu")
        app.wait(lambda t: count() == expanded)
        # Synthetic controls are added only after all recorded summaries have completed.
        long_reply = {"id": 90000001, "parent": 50163214, "author": "synthetic_replay_control",
                      "text": "\n".join(f"Synthetic passage {i:03} · revision 1" for i in range(260))}
        records.insert(0, long_reply)
        app.key("rg")
        app.wait(lambda t: count() == initial + 3)
        app.key("c")
        app.wait(lambda t: "+ 4 replies" in t and count() == initial)
        app.key("\r")
        app.wait(lambda t: count() == initial + 3)
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

        # Load one branch past the cap without changing the recorded summary inputs.
        base = 90000100
        cap = max(2, min(6, (width - 2 - 60) // 2 + 1))
        def chain_author(level):
            return f"chain_author_{level:02}"

        records[0:0] = [
            {"id": base + level, "parent": base + level - 1 if level > 1 else None,
             "author": chain_author(level), "text": f"Synthetic chain level {level}."}
            for level in range(1, 13)
        ] + [{"id": base + 20, "parent": base + 1, "author": "chain_side",
              "text": "Synthetic sibling outside the selected chain."}]
        app.key("rg")
        app.wait(lambda t: count() == initial + 5)
        app.key("g\r")
        app.wait(lambda t: "chain_side" in t)
        app.key("j")
        selected(chain_author(2), 1)
        side_row = next(i for i, line in enumerate(app.screen.display) if "│ chain_side  " in line)
        assert app.screen.buffer[side_row][4].fg == "e1e5e5"
        app.key("f")
        assert app.screen.buffer[side_row][4].fg == "a1acb0", "Focus did not dim the sibling"
        selected_row = next(i for i, line in enumerate(app.screen.display)
                            if line.startswith(f"▎ │ {chain_author(2)}  "))
        assert app.screen.buffer[selected_row][4].fg == "e1e5e5", "Focus dimmed the selected chain"
        capture("15a-thread-focus")
        app.key("f")
        assert app.screen.buffer[side_row][4].fg == "e1e5e5"
        for level in range(3, cap + 1):
            app.key("\r")
            app.wait(lambda t: f"{chain_author(level)}  " in t)
            app.key("j")
        selected(chain_author(cap), cap - 1)
        assert "continue thread → 1 reply" in app.text()
        assert f"{chain_author(cap + 1)}  " not in app.text()
        capture("15b-thread-cap")
        app.key("\r")
        app.wait(lambda t: f"{chain_author(cap + 1)}  " in t and f"› {chain_author(cap)}" in t)
        selected(chain_author(cap), 0)
        expected_trail = " › ".join(chain_author(level) for level in range(1, cap + 1))
        if len(expected_trail) > width:
            expected_trail = "…" + expected_trail[-(width - 1):]
        assert any(line.rstrip() == expected_trail for line in app.screen.display)
        capture("15c-thread-trail")
        app.key("\x7f")
        selected(chain_author(cap - 1), 0)
        assert any(line.rstrip().endswith(chain_author(cap - 1)) for line in app.screen.display)
        capture("15d-thread-backspace")
        app.key("jj")
        for level in range(cap + 2, 2 * cap - 1):
            app.key("\r")
            app.wait(lambda t: f"{chain_author(level)}  " in t)
            app.key("j")
        selected(chain_author(2 * cap - 2), cap - 1)
        app.key("o")
        app.wait(lambda t: f"{chain_author(2 * cap - 1)}  " in t)
        selected(chain_author(2 * cap - 2), 0)
        capture("15e-thread-o")

        app.key("\x1b")
        app.wait(lambda t: "4 Saved" in t)
        app.key("1")
        app.wait(lambda t: "1 Top" in t and "Synthetic feed control 01" in t)
        for page in (2, 3):
            app.key("n")
            app.wait(lambda t: re.search(rf"Page {page}  $", t, re.M) and f"Synthetic feed control {(page - 1) * 20 + 1:02}" in t)
        app.key("j" * 19 + "s")
        app.wait(lambda t: "▎  60  Synthetic feed control 60 ★" in t)
        feed = app.text().splitlines()[3:-2]
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
        assert app.text().splitlines()[3:-2] == feed, "Feed return lost its scroll position"
        assert fixture.gets.count("/v0/topstories.json") == 3, "Feed return fetched again"
        capture("17-feed-restored")
        app.key("s4")
        app.wait(lambda t: "4 Saved" in t and "Synthetic feed control 60" not in t)
        assert re.search(r"▎   1  .*Nordwand Mono", app.text()), "Saved removal did not select the remaining story"
        capture("18-saved-current")

        left, right = "\x1b[D", "\x1b[C"

        def selected_story(rank, column):
            marker = f"▎ {rank:>3}  "
            app.wait(lambda text: any(marker in row and (row.index(marker) > 0) == column
                                      for row in text.splitlines()))

        def resize(columns):
            app.screen.resize(lines=height, columns=columns)
            fcntl.ioctl(app.fd, termios.TIOCSWINSZ, struct.pack("HHHH", height, columns, 0, 0))
            app.wait(lambda text: ("←/→ column / page" in text) == (columns >= 160))

        app.key("1")
        selected_story(60, width >= 160)
        if width < 160:
            app.key(left)
            selected_story(21, False)
            app.key(right)
            selected_story(41, False)
            resize(160)
        app.key("g" + right)
        selected_story(51, True)
        assert re.search(r"Page 3  $", app.text(), re.M)
        capture("19-feed-right-column")
        app.key(left + left)
        selected_story(40, True)
        assert re.search(r"Page 2  $", app.text(), re.M)
        capture("20-feed-previous-edge")
        app.key(right)
        selected_story(41, False)
        assert re.search(r"Page 3  $", app.text(), re.M)
        capture("21-feed-next-edge")
        app.key(right)
        selected_story(51, True)
        resize(159)
        selected_story(51, False)
        capture("22-feed-resize-narrow")
        resize(160)
        selected_story(51, True)
        capture("23-feed-resize-wide")
        assert fixture.gets.count("/v0/topstories.json") == 3, "Arrow paging or resize fetched a cached page"

        app.key("ppg")
        selected_story(1, False)
        app.key("sj" * 19 + "sn")
        selected_story(21, False)
        app.key("s4")
        app.wait(lambda t: "Nordwand Mono" in t and "←/→ column " in t)
        selected_story(22, True)
        app.key("g" + right)
        selected_story(12, True)
        assert "▎  12  Synthetic feed control 10" in app.text()
        capture("24-saved-two-columns")
        app.key(right)
        selected_story(12, True)
        app.key("s")
        selected_story(12, True)
        assert "▎  12  Synthetic feed control 09" in app.text()
        assert "Synthetic feed control 10" not in app.text()
        capture("25-saved-after-removal")
        assert fixture.gets.count("/v0/topstories.json") == 3, "Saved unexpectedly fetched a page"

        app.key("1pgc")
        app.wait(lambda t: count() == 2 and "ordinary_sibling  " in t)
        app.key("g\r")
        app.wait(lambda t: count() == 3)
        selected("ordinary_parent", 0)
        parent_row = next(i for i, line in enumerate(app.screen.display) if "ordinary_parent  " in line)
        app.key("j")
        selected("ordinary_reply", 1)
        assert any(line.rstrip() == "ordinary_parent › ordinary_reply" for line in app.screen.display)
        assert "ordinary_parent  " in app.screen.display[parent_row], "Trail changed the position of the discussion"
        capture("26-discussion-ordinary-trail")
        app.key("\x7f")
        selected("ordinary_parent", 0)
        app.wait(lambda t: count() == 2 and "ordinary_sibling  " not in t)
        assert "ordinary_parent › ordinary_reply" not in app.text()
        capture("27-discussion-ordinary-backspace")
        app.quit()
        assert not fixture.errors, fixture.errors
        receipt = {"size": [width, height], "provider_calls": fixture.calls,
                   "visible_thread_levels": cap,
                   "reply_counts": [initial, expanded, collapsed], "captures": captured,
                   "reply_get_counts": {"reopen": dict(reopen), "late_after_collapse": dict(late), "citation": dict(source_requests)},
                   "passage_anchors": {"before_refresh": before[0], "during_and_after_refresh": continued[0]},
                   "hn_get_counts": {path: fixture.gets.count(path) for path in sorted(set(fixture.gets))},
                   "exit": "clean", "network": "Loopback fixtures; external HTTP blocked"}
        (output / "result.json").write_text(json.dumps(receipt, indent=2) + "\n")
        print(f"PASS {width}×{height}: recorded summaries, citation replies and retry, comment refresh, {cap}-level cap, trail, focus, feed/Saved continuity, clean exit", flush=True)
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
