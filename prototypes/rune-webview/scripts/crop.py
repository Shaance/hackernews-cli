#!/usr/bin/env python3
"""Crop a fullscreen capture to a window frame and assert the result has real content.

`screencapture -l <CGWindowID>` returns an empty buffer for a Metal-backed window in this
environment, so verification captures the whole screen and crops here instead.

Usage: crop.py FULL X Y W H OUT [--scale 2] [--expect-orange]

Prints one line of evidence per image and exits non-zero if the crop is blank (more than 99% a
single colour), which is what a failed capture looks like.
"""
import sys

from PIL import Image

# HN's header orange (#ff6600). The capture is colour-managed, so it lands near (237, 112, 45)
# rather than the literal value; the tolerance covers that shift plus antialiasing.
ORANGE = (255, 102, 0)
ORANGE_TOLERANCE = 60


def main(argv):
    full, x, y, w, h, out = argv[1:7]
    x, y, w, h = int(x), int(y), int(w), int(h)
    scale = 2.0
    expect_orange = False
    rest = argv[7:]
    i = 0
    while i < len(rest):
        if rest[i] == "--scale":
            scale = float(rest[i + 1])
            i += 2
        elif rest[i] == "--expect-orange":
            expect_orange = True
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

    print(line)
    if blank:
        raise SystemExit(f"crop: {out} is blank ({top_share:.1%} one colour)")


if __name__ == "__main__":
    main(sys.argv)
