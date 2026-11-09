#!/usr/bin/env python3
"""Crop a fullscreen capture to a window frame and assert the article pane holds a real page.

`screencapture -l <CGWindowID>` returns an empty buffer for a Metal-backed window in this
environment, so verification captures the whole screen and crops here instead.

Usage: crop.py FULL X Y W H OUT [--scale 2] [--pane] [--expect-orange]

Prints one line of evidence per image and exits non-zero when the crop is blank (more than 99%
one colour), which is what a failed capture looks like.

`--pane` adds the assertion this change exists for: the article pane is the live page, not the
extracted text. It samples the app's own background from the window's top strip (Rune's toolbar,
which no child view covers) and requires that colour to cover less than half of the pane region.
The extracted-text fallback is that background with sparse text on it — around 85% — while a
rendered page paints its own background over the whole region. It also requires real content:
more than 500 distinct colours, which a page's text antialiasing and images produce and a
mostly-empty Rune pane does not.
"""
import sys

from PIL import Image

# HN's header orange (#ff6600). The capture is colour-managed, so it lands near (237, 112, 45)
# rather than the literal value; the tolerance covers that shift plus antialiasing.
ORANGE = (255, 102, 0)
ORANGE_TOLERANCE = 60
# How far a pixel may sit from the sampled app background and still count as it.
BACKGROUND_TOLERANCE = 12


def main(argv):
    full, x, y, w, h, out = argv[1:7]
    x, y, w, h = int(x), int(y), int(w), int(h)
    scale = 2.0
    expect_orange = False
    pane = False
    rest = argv[7:]
    i = 0
    while i < len(rest):
        if rest[i] == "--scale":
            scale = float(rest[i + 1])
            i += 2
        elif rest[i] == "--expect-orange":
            expect_orange = True
            i += 1
        elif rest[i] == "--pane":
            pane = True
            i += 1
        else:
            raise SystemExit(f"crop: unknown argument {rest[i]!r}")

    image = Image.open(full).convert("RGB")
    box = (
        int(x * scale),
        int(y * scale),
        int((x + w) * scale),
        int((y + h) * scale),
    )
    box = (
        max(0, box[0]),
        max(0, box[1]),
        min(image.width, box[2]),
        min(image.height, box[3]),
    )
    if box[2] <= box[0] or box[3] <= box[1]:
        raise SystemExit(f"crop: window frame {x},{y} {w}x{h} is outside the {image.width}x{image.height} capture")

    crop = image.crop(box)
    crop.save(out)

    colours = sorted(crop.getcolors(maxcolors=crop.width * crop.height), reverse=True)
    top_share = colours[0][0] / (crop.width * crop.height)
    blank = top_share > 0.99

    line = (
        f"{out}  {crop.width}x{crop.height}  distinct={len(colours)}  "
        f"top={colours[0][1]} {top_share:.1%}  {'BLANK' if blank else 'content'}"
    )

    if expect_orange:
        orange = sum(
            count
            for count, colour in colours
            if all(abs(colour[c] - ORANGE[c]) <= ORANGE_TOLERANCE for c in range(3))
        )
        line += f"  hn-orange={orange}px"
        if orange < 1000:
            print(line)
            raise SystemExit(f"crop: expected HN orange in {out}, found {orange}px (<1000)")

    if pane:
        # The app's own background, from the window's top strip: Rune's toolbar, which the child
        # view never covers. The most common colour there is the theme's background.
        strip = image.crop((box[0], box[1], box[2], box[1] + int(8 * scale)))
        background = sorted(strip.getcolors(maxcolors=strip.width * strip.height), reverse=True)[0][1]
        # The article pane: the left half of the window, clear of the toolbar above and the
        # story footer below. The inspector is the right 46%.
        region = crop.crop(
            (
                0,
                int(crop.height * 0.15),
                int(crop.width * 0.5),
                int(crop.height * 0.85),
            )
        )
        pixels = list(region.getdata())
        same = sum(
            1
            for pixel in pixels
            if all(abs(pixel[c] - background[c]) <= BACKGROUND_TOLERANCE for c in range(3))
        )
        share = same / len(pixels)
        line += f"  app-bg={background} {share:.1%} of pane"
        if share >= 0.5:
            print(line)
            raise SystemExit(
                f"crop: the article pane is the app's own background ({share:.1%} of it), "
                f"not a rendered page"
            )
        if len(colours) <= 500:
            print(line)
            raise SystemExit(f"crop: only {len(colours)} distinct colours in {out}; no page content")

    print(line)
    if blank:
        raise SystemExit(f"crop: {out} is blank ({top_share:.1%} one colour)")


if __name__ == "__main__":
    main(sys.argv)
