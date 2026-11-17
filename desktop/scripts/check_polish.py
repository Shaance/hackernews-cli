#!/usr/bin/env python3
"""Exercise reading transitions and layout through the desktop headless entrypoint."""
import argparse
import copy
import json
import os
from pathlib import Path
import subprocess


ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--bin", type=Path, default=ROOT / "target/debug/hn-desktop")
parser.add_argument("--output", type=Path, default=ROOT / "target/polish-fix/rendered")
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)


def key(t, name):
    return {"t": t, "type": "key", "name": name}


def capture(name, scene="story", events=None, at=.3, size="1180x760",
            theme="dark", reduced=False):
    stem = args.output / name
    command = [str(args.bin.resolve()), "--scene", scene, "--size", size,
               "--scale", "1", "--theme", theme, "--at", str(at),
               "--snap", str(stem.with_suffix(".png").resolve()),
               "--tree", str(stem.with_suffix(".json").resolve()),
               "--trace", str(stem.with_suffix(".jsonl").resolve())]
    if events:
        script = stem.with_suffix(".input.json")
        script.write_text(json.dumps(events))
        command += ["--script", str(script.resolve())]
    env = os.environ.copy()
    env["HN_REDUCED_MOTION"] = "1" if reduced else "0"
    with stem.with_suffix(".log").open("w") as log:
        subprocess.run(command, cwd=ROOT, env=env, stdout=log,
                       stderr=subprocess.STDOUT, check=True)
    return json.loads(stem.with_suffix(".json").read_text())["elements"]


def visible(elements, key_name):
    return [e for e in elements if e.get("key") == key_name
            and not e.get("exiting") and e.get("opacity", 1) > 0]


def one(elements, key_name):
    found = visible(elements, key_name)
    assert len(found) == 1, (key_name, len(found))
    return found[0]


def no_reading_ghosts(elements):
    ghosts = [e.get("key") for e in elements if e.get("exiting")
              and e.get("text") and e.get("opacity", 1) > .001
              and "/inspector-pane/" in e.get("path", "")]
    assert not ghosts, f"outgoing reading text remains painted: {ghosts}"


def verify_arrival(first, second, section):
    a, b = (one(frame, f"{section}-summary-result") for frame in (first, second))
    assert .4 < a["opacity"] < b["opacity"] < 1, (section, "opacity", a, b)
    assert a["rect"][1] > b["rect"][1] + 1, (section, "rise", a["rect"], b["rect"])


def verify_navigation(frames, source_x, rest_x):
    source_50, source_100 = (one(frame, "source-viewer") for frame in frames[:2])
    back_50, back_100 = (one(frame, "inspector") for frame in frames[2:])
    hidden = [e for e in frames[0] if e.get("key") == "inspector" and not e.get("exiting")]
    assert len(hidden) == 1 and hidden[0]["inert"] and hidden[0]["opacity"] == 0
    assert source_50["rect"][0] > source_100["rect"][0] > source_x + 1
    assert back_50["rect"][0] < back_100["rect"][0] < rest_x - 1
    assert source_50["opacity"] < source_100["opacity"] < 1
    assert back_50["opacity"] < back_100["opacity"] < 1


def verify_footer(frame, reference):
    def labels(elements):
        return [e for e in elements if "/story-footer/" in e.get("path", "")
                and e.get("text") and e.get("opacity", 1) > .001]

    actual, settled = labels(frame), labels(reference)
    assert not any(e.get("exiting") for e in actual), "outgoing footer text remains painted"
    assert [(e["path"], e["text"]) for e in actual] == [(e["path"], e["text"]) for e in settled]
    for a, b in zip(actual, settled):
        assert all(abs(x - y) < .1 for x, y in zip(a["rect"], b["rect"])), (a["text"], a["rect"], b["rect"])
    for i, a in enumerate(actual):
        ax, ay, aw, ah = a["rect"]
        for b in actual[i + 1:]:
            bx, by, bw, bh = b["rect"]
            assert ax + aw <= bx or bx + bw <= ax or ay + ah <= by or by + bh <= ay, (a["text"], b["text"])


def verify_citations(elements):
    links = [e for e in elements if str(e.get("key", "")).startswith("cite-")]
    assert len(links) == 5
    assert all(e["role"] == "button" and "summary-text-" in e["path"] for e in links)
    assert links[0]["label"] == "Read comment by tptacek"


def rejects_mutation(name, value, check):
    path = args.output / f"mutation-{name}.json"
    path.write_text(json.dumps(value))
    try:
        check(json.loads(path.read_text()))
    except AssertionError:
        return
    raise AssertionError(f"validator accepted {name} mutation")


summary = [key(.1, "S"), key(.8, "S")]
citations = [key(.1, "Tab"), key(.2, "Enter"), key(.8, "Escape")]
for reduced in [False, True]:
    mode = "reduced" if reduced else "normal"
    for theme in ["dark", "light"]:
        resting = capture(f"citation-{mode}-{theme}-rest", theme=theme, reduced=reduced)
        rest_x = one(resting, "inspector")["rect"][0]
        opened = capture(f"citation-{mode}-{theme}-open", events=citations,
                         at=.6, theme=theme, reduced=reduced)
        source_x = one(opened, "source-viewer")["rect"][0]
        for action, events, times in [
            ("summary", summary, [.15, .2, .85, .9]),
            ("citation", citations, [.25, .3, .85, .9]),
        ]:
            fades, positions, frames = [], [], []
            for at in times:
                elements = capture(f"{action}-{mode}-{theme}-{round(at*1000)}",
                                   events=events, at=at, theme=theme, reduced=reduced)
                frames.append(elements)
                no_reading_ghosts(elements)
                if action == "summary":
                    node = one(elements, "summary-title")
                    assert bool(visible(elements, "summary-text-0")) == (at > .8)
                else:
                    node = one(elements, "source-viewer" if at < .8 else "inspector")
                    assert not visible(elements, "inspector" if at < .8 else "source-viewer")
                    verify_footer(elements, opened if at < .8 else resting)
                    if at < .8:
                        hidden = [e for e in elements if e.get("key") == "inspector" and not e.get("exiting")]
                        assert len(hidden) == 1 and hidden[0]["inert"] and hidden[0]["opacity"] == 0
                fades.append(node["opacity"])
                positions.append(node["rect"][0])
                if reduced:
                    assert node["opacity"] == 1, (action, "reduced opacity", node["opacity"])
            if not reduced:
                assert fades[0] < fades[1] < 1, (action, "entrance", fades)
                assert fades[2] < fades[3] < 1, (action, "return", fades)
                if action == "citation":
                    verify_navigation(frames, source_x, rest_x)
                    if theme == "dark":
                        mutant = copy.deepcopy(frames[0])
                        next(e for e in mutant if e.get("text") == "Back to discussion" and "/story-footer/" in e.get("path", ""))["rect"][2] -= 12
                        rejects_mutation("moving-footer-label", mutant, lambda value: verify_footer(value, opened))
                        mutant = copy.deepcopy(frames[0])
                        ghost = copy.deepcopy(next(e for e in resting if e.get("text") == "refresh comments"))
                        ghost["exiting"] = True
                        mutant.append(ghost)
                        rejects_mutation("outgoing-footer-label", mutant, lambda value: verify_footer(value, opened))
                        mutant = copy.deepcopy(frames)
                        next(e for e in mutant[0] if e.get("key") == "inspector")["inert"] = False
                        rejects_mutation("active-hidden-inspector", mutant,
                                         lambda value: verify_navigation(value, source_x, rest_x))
                        mutant = copy.deepcopy(frames)
                        for frame in mutant[:2]:
                            next(e for e in frame if e.get("key") == "source-viewer")["rect"][0] = source_x
                        rejects_mutation("instant-source", mutant,
                                         lambda value: verify_navigation(value, source_x, rest_x))
            elif action == "citation":
                assert positions == [source_x, source_x, rest_x, rest_x], positions
        print(f"PASS readable summary/citation handoffs: {mode}, {theme}", flush=True)

arrivals = ROOT / "target/ux-motion/reading-arrivals"
for section, times in [("article", ("025", "030")), ("discussion", ("075", "080"))]:
    for theme in ["dark", "light"]:
        first, second = [json.loads((arrivals / f"{section}-normal-{theme}-{at}.json").read_text())["elements"] for at in times]
        verify_arrival(first, second, section)
        if theme == "dark":
            mutant = copy.deepcopy([first, second])
            for frame in mutant:
                next(e for e in frame if e.get("key") == f"{section}-summary-result")["opacity"] = 1
            rejects_mutation(f"instant-{section}", mutant,
                             lambda value: verify_arrival(value[0], value[1], section))
        reduced_frames = [json.loads((arrivals / f"{section}-reduced-{theme}-{at}.json").read_text())["elements"] for at in times]
        assert all(one(frame, f"{section}-summary-result")["opacity"] == 1 for frame in reduced_frames)
print("PASS async section arrivals and sensitivity to instant motion", flush=True)

for name, events in [
    ("summary", [key(t, "S") for t in [.1, .16, .22, .3]]),
    ("citation", [key(.1, "Tab"), key(.2, "Enter"), key(.25, "Escape"),
                  key(.3, "Enter"), key(.35, "Escape")]),
]:
    for at in [.45, .9]:
        elements = capture(f"{name}-rapid-{round(at*1000)}", events=events, at=at)
        no_reading_ghosts(elements)
        one(elements, "inspector")
        one(elements, "summary-text-0")
    print(f"PASS {name} rapid reversal", flush=True)

for theme in ["dark", "light"]:
    for scene in ["help", "settings"]:
        elements = capture(f"{scene}-short-{theme}", scene=scene, size="1040x560", theme=theme)
        panel = one(elements, f"{scene}-dialog")["rect"]
        assert panel[0] >= 16 and panel[1] >= 16, (scene, panel)
        assert panel[0] + panel[2] <= 1024 and panel[1] + panel[3] <= 544, (scene, panel)
        close = one(elements, "close")["rect"]
        assert close[1] >= panel[1] and close[1] + close[3] <= panel[1] + panel[3]
        content = one(elements, "help-columns" if scene == "help" else "settings-fields")["rect"]
        assert content[0] >= panel[0] + 16, (scene, "left inset", content, panel)
        assert content[0] + content[2] <= panel[0] + panel[2] - 16, (scene, "right inset")
    print(f"PASS bounded, inset short dialogs: {theme}", flush=True)

top = capture("help-scroll-top", scene="help", size="1040x560")
scrolled = capture("help-scroll-bottom", scene="help", size="1040x560", at=.9,
                   events=[{"t": .2, "type": "scroll", "x": 600, "y": 350,
                            "dy": -400, "dur": .3}])
assert one(scrolled, "help-columns")["rect"][1] < one(top, "help-columns")["rect"][1]
viewport = one(top, "help-scroll")["rect"]
def reachable_rows(elements):
    return {e["path"] for e in elements if str(e.get("key", "")).startswith("help-row-")
            and e["rect"][1] >= viewport[1] - .01
            and e["rect"][1] + e["rect"][3] <= viewport[1] + viewport[3] + .01}
all_rows = {e["path"] for e in top if str(e.get("key", "")).startswith("help-row-")}
assert len(all_rows) == 47 and reachable_rows(top) | reachable_rows(scrolled) == all_rows
for fixed in ["close", "help-footer"]:
    assert one(top, fixed)["rect"] == one(scrolled, fixed)["rect"], fixed
resized = capture("help-resized-short", scene="help", at=.9,
                  events=[{"t": .2, "type": "resize", "dw": 0, "dh": -200, "dur": .3}])
panel = one(resized, "help-dialog")["rect"]
assert panel[1] >= 16 and panel[1] + panel[3] <= 544, panel
print("PASS help scroll and live resize keep controls visible", flush=True)

for size in ["1180x760", "1600x900"]:
    before = capture(f"save-{size}-before", scene="feed", at=.05, size=size)
    after = capture(f"save-{size}-after", scene="feed", events=[key(.1, "s")], size=size)
    for name in ["title-1", "rank-1", "story-1"]:
        assert one(before, name)["rect"] == one(after, name)["rect"], (name, "save moved layout")
    print(f"PASS saved row stays fixed: {size}", flush=True)

story = capture("article-button")
verify_citations(story)
mutant = [e for e in story if e.get("key") != "cite-0"]
rejects_mutation("missing-citation", mutant, verify_citations)
button = one(story, "article-open-original")
assert button["role"] == "button"
assert "/story-footer/" in button["path"], button["path"]
assert not visible(story, "story-toolbar-title")
assert not visible(story, "story-toolbar-meta")
assert not visible(story, "summary-pill")
assert any(e.get("text") == "Back to feed" and "/story-footer/" in e.get("path", "") for e in story)
title = one(story, "article-title")
assert sum(e.get("text") == title["text"] for e in story) == 1
assert abs(one(story, "main-pane")["rect"][1] - one(story, "summary-card")["rect"][1]) < .5
print("PASS single story heading, aligned panes, compact browser action", flush=True)
