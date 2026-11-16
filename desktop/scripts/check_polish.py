#!/usr/bin/env python3
"""Exercise reading transitions and layout through the desktop headless entrypoint."""
import argparse
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


summary = [key(.1, "S"), key(.8, "S")]
citations = [key(.1, "Tab"), key(.2, "Enter"), key(.8, "Escape")]
for reduced in [False, True]:
    mode = "reduced" if reduced else "normal"
    for theme in ["dark", "light"]:
        for action, events, times in [
            ("summary", summary, [.15, .2, .85, .9]),
            ("citation", citations, [.25, .3, .85, .9]),
        ]:
            fades = []
            for at in times:
                elements = capture(f"{action}-{mode}-{theme}-{round(at*1000)}",
                                   events=events, at=at, theme=theme, reduced=reduced)
                no_reading_ghosts(elements)
                if action == "summary":
                    node = one(elements, "summary-title")
                    assert bool(visible(elements, "summary-text-0")) == (at > .8)
                else:
                    node = one(elements, "source-viewer" if at < .8 else "summary-title")
                    assert not visible(elements, "inspector" if at < .8 else "source-viewer")
                fades.append(node["opacity"])
                if reduced:
                    assert node["opacity"] == 1, (action, "reduced opacity", node["opacity"])
            if not reduced:
                assert .65 <= fades[0] < fades[1] < 1, (action, "entrance", fades)
                assert .65 <= fades[2] < fades[3] < 1, (action, "return", fades)
        print(f"PASS readable summary/citation handoffs: {mode}, {theme}", flush=True)

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
button = one(story, "article-open-original")
assert button["rect"][3] == 24, button["rect"]
assert button["role"] == "button"
assert not visible(story, "story-toolbar-title")
assert not visible(story, "story-toolbar-meta")
assert not visible(story, "summary-pill")
one(story, "story-back")
title = one(story, "article-title")
assert sum(e.get("text") == title["text"] for e in story) == 1
assert abs(one(story, "main-pane")["rect"][1] - one(story, "summary-card")["rect"][1]) < .5
print("PASS single story heading, aligned panes, compact browser action", flush=True)
