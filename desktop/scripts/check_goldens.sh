#!/usr/bin/env bash
# The headless golden gate for hn-desktop (DESKTOP_IMPLEMENTATION_PLAN.md v4, "Acceptance gate").
#
# For every frozen scenario it renders the app offscreen, then checks four independent things:
#
#   1. the element tree, canonicalised with `jq -S`, is byte-identical to expected/;
#   2. the PNG is within 0.5% of expected/ (macOS only: cross-platform pixel equality is not
#      claimed, so Linux and Windows run with HN_TREES_ONLY=1);
#   3. the motion lint: nothing animates at rest, and no spatial track is ever active;
#   4. fidelity invariants read from the tree, not the pixels — column x-positions, rank order,
#      stepper text, and the selected row's focus flags. These exist so a self-generated golden
#      cannot ratify a wrong layout.
#
# Usage:  bash desktop/scripts/check_goldens.sh
#   HN_TREES_ONLY=1   skip the PNG comparison (Linux/Windows CI)
#   HN_SKIP_BUILD=1   use the binary already in target/debug
set -euo pipefail

cd "$(dirname "$0")/.."

EXPECTED=verification/expected
ACTUAL=verification/actual
BIN=target/debug/hn-desktop
TREES_ONLY=${HN_TREES_ONLY:-0}
# A frame is at rest once the last scripted event has settled. The resize script's last
# animation ends at 0.6 s, so one second after launch nothing may still be moving.
AT_REST_MS=1000
# Spatial properties: anything that moves or resizes. A fade is not one of them.
SPATIAL='frame,offset,scale,rotation,travel,scroll'

fail=0
pass() { printf '  ok    %s\n' "$*"; }
bad() { printf '  FAIL  %s\n' "$*"; fail=1; }
step() { printf '\n== %s\n' "$*"; }

for tool in jq; do
  command -v "$tool" >/dev/null || { echo "missing $tool"; exit 2; }
done
if [ "$TREES_ONLY" != 1 ]; then
  for tool in compare identify; do
    command -v "$tool" >/dev/null || { echo "missing $tool (ImageMagick); set HN_TREES_ONLY=1 to skip pixels"; exit 2; }
  done
fi

mkdir -p "$ACTUAL"

if [ "${HN_SKIP_BUILD:-0}" != 1 ]; then
  step "build"
  cargo build --locked --bin hn-desktop
fi

# name | scene | size | script | columns
# `resize` starts at 1600 and ends at 1100, so its golden is the one-column end state.
SCENES=(
  "feed-1040|feed|1040x760|scripts/focus.json|1"
  "feed|feed|1180x760|scripts/focus.json|1"
  "feed-wide|feed-wide|1600x900|scripts/focus.json|2"
  "feed-saved|feed-saved|1180x760|scripts/focus.json|1"
  "resize|resize|1600x900|scripts/resize.json|1"
)

# name | scene | size | script
# The story split's three frozen states. They share the render/tree/pixel/motion loops with the
# feed scenes but not the feed's fidelity loop: a story has no rank order and no page stepper.
STORY_SCENES=(
  "story|story|1180x760|scripts/story.json"
  "citation|citation|1180x760|scripts/story.json"
  "summary-generating|summary-generating|1180x760|scripts/story.json"
)

ALL=("${SCENES[@]}" "${STORY_SCENES[@]}")

# A scene whose golden is an indeterminate-progress state may keep one loop running: the
# summary card's spinner. Nothing else may still be moving, and the loop may not move anything
# spatially.
allows_loop() { [ "$1" = "summary-generating" ]; }

render() { # name scene size script
  "$BIN" --scene "$2" --size "$3" --scale 2 --theme dark --script "$4" \
    --snap "$ACTUAL/$1.png" --tree "$ACTUAL/$1.json" --trace "$ACTUAL/$1.trace.jsonl" >/dev/null
}

step "render"
for row in "${ALL[@]}"; do
  IFS='|' read -r name scene size script _ <<<"$row"
  render "$name" "$scene" "$size" "$script"
  pass "$name ($scene @ $size)"
done

step "trees (jq -S canonical diff must be empty)"
for row in "${ALL[@]}"; do
  IFS='|' read -r name _ _ _ _ <<<"$row"
  jq -S . "$EXPECTED/$name.json" >"$ACTUAL/$name.expected.canon.json"
  jq -S . "$ACTUAL/$name.json" >"$ACTUAL/$name.actual.canon.json"
  if diff -q "$ACTUAL/$name.expected.canon.json" "$ACTUAL/$name.actual.canon.json" >/dev/null; then
    pass "$name tree"
  else
    bad "$name tree"
    diff -u "$ACTUAL/$name.expected.canon.json" "$ACTUAL/$name.actual.canon.json" | head -40 || true
  fi
done

if [ "$TREES_ONLY" = 1 ]; then
  step "pixels (skipped: HN_TREES_ONLY=1)"
else
  step "pixels (compare -metric AE -fuzz 2%, fail above 0.5%)"
  for row in "${ALL[@]}"; do
    IFS='|' read -r name _ _ _ _ <<<"$row"
    read -r w h <<<"$(identify -format '%w %h' "$EXPECTED/$name.png")"
    total=$((w * h))
    # `compare` exits 1 when the images differ at all, so the metric is read from stderr.
    ae=$(compare -metric AE -fuzz 2% "$EXPECTED/$name.png" "$ACTUAL/$name.png" \
      "$ACTUAL/$name.diff.png" 2>&1 >/dev/null || true)
    ae=${ae%% *}
    case "$ae" in
      ''|*[!0-9]*) bad "$name png (unreadable metric: $ae)"; continue ;;
    esac
    # 0.5% of pixels, as an integer comparison: ae * 1000 > total * 5.
    if [ $((ae * 1000)) -le $((total * 5)) ]; then
      pass "$name png ($ae of $total px differ)"
    else
      bad "$name png ($ae of $total px differ, over 0.5%; see $ACTUAL/$name.diff.png)"
    fi
  done
fi

step "motion lint"
for row in "${ALL[@]}"; do
  IFS='|' read -r name _ _ _ _ <<<"$row"
  trace="$ACTUAL/$name.trace.jsonl"

  # Nothing may still be moving a second after launch, except a scene's declared progress loop.
  if allows_loop "$name"; then
    moving=$(jq -s --argjson at "$AT_REST_MS" \
      '[.[] | select(.quality.animating == true and .t >= $at)
        | select((.quality.why // []) != ["loop"])] | length' "$trace")
    if [ "$moving" = 0 ]; then
      pass "$name at rest (only the progress loop after ${AT_REST_MS}ms)"
    else
      bad "$name at rest ($moving animating frames beyond the progress loop after ${AT_REST_MS}ms)"
    fi
  else
    moving=$(jq -s --argjson at "$AT_REST_MS" \
      '[.[] | select(.quality.animating == true and .t >= $at)] | length' "$trace")
    if [ "$moving" = 0 ]; then
      pass "$name at rest (0 animating frames after ${AT_REST_MS}ms)"
    else
      bad "$name at rest ($moving animating frames after ${AT_REST_MS}ms)"
    fi
  fi

  # Only intended tracks: `tk` is [id, prop, active]; active is 1 while the track moves. A loop
  # frame carries no `tk` at all, so the list is read as empty rather than as an error.
  spatial=$(jq -s --arg spatial "$SPATIAL" \
    '[.[] | select(.quality.animating == true) | (.tk // [])[] | select(.[2] == 1)
      | select((.[1] | ascii_downcase) as $p | ($spatial | split(",") | index($p)) != null)] | length' "$trace")
  if [ "$spatial" = 0 ]; then
    pass "$name tracks (no active spatial track)"
  else
    bad "$name tracks ($spatial active spatial tracks)"
  fi

  props=$(jq -rs '[.[] | select(.quality.animating == true) | (.tk // [])[] | select(.[2] == 1) | .[1]] | unique | join(",")' "$trace")
  printf '        %s active props: [%s]\n' "$name" "$props"
done

step "fidelity (read from the tree, independent of pixels)"
for row in "${SCENES[@]}"; do
  IFS='|' read -r name _ size _ want <<<"$row"
  tree="$ACTUAL/$name.json"

  # Column x-positions: one column below the 1400pt breakpoint, two at or above it.
  cols=$(jq '[.elements[] | select((.key // "") | startswith("story-")) | .rect[0]] | unique | length' "$tree")
  if [ "$cols" = "$want" ]; then
    pass "$name columns ($cols at $size)"
  else
    bad "$name columns ($cols at $size, want $want)"
  fi

  # Rank order and column-major placement. The fixture's Top ids are 1..30 in rank order, so
  # reading the story keys in (x, y) order must give 1..N: down the first column, then the next.
  order=$(jq -r '[.elements[] | select((.key // "") | startswith("story-"))
      | {x: .rect[0], y: .rect[1], id: (.key | sub("story-"; "") | tonumber)}]
    | sort_by(.x, .y) | map(.id) | @csv' "$tree")
  n=$(jq '[.elements[] | select((.key // "") | startswith("story-"))] | length' "$tree")
  want_order=$(seq 1 "$n" | paste -sd, -)
  if [ "$order" = "$want_order" ]; then
    pass "$name rank order (column-major 1..$n)"
  else
    bad "$name rank order (got $order, want $want_order)"
  fi

  # The rank labels agree with the keys.
  ranks=$(jq -r '[.elements[] | select((.key // "") | startswith("rank-")) | .text | tonumber] | sort | @csv' "$tree")
  if [ "$ranks" = "$want_order" ]; then
    pass "$name rank labels (1..$n)"
  else
    bad "$name rank labels (got $ranks)"
  fi

  # The stepper shows the page and nothing else: no "of N", because the API reports no total.
  stepper=$(jq -r '[.elements[] | select((.key // "") == "page-stepper") | .label][0] // ""' "$tree")
  if [ "$stepper" = "Page 1" ]; then
    pass "$name stepper (Page 1)"
  else
    bad "$name stepper (got '$stepper')"
  fi

  # The selected row is a Tab stop and holds focus.
  focus=$(jq -r '[.elements[] | select((.key // "") == "story-1") | "\(.focusable)/\(.focused)"][0] // ""' "$tree")
  if [ "$focus" = "true/true" ]; then
    pass "$name selected row (focusable + focused)"
  else
    bad "$name selected row (focusable/focused = '$focus')"
  fi
done

step "saved view (save-time order, hover unsave, corrupt banner)"
# These two scenes are not in the frozen golden matrix; they assert the Saved view's three
# required behaviours directly, so a golden cannot ratify a wrong order or a missing banner.
"$BIN" --scene feed-saved-view --size 1180x760 --scale 2 --theme dark --script scripts/focus.json \
  --tree "$ACTUAL/saved-view.json" >/dev/null
"$BIN" --scene feed-saved-corrupt --size 1180x760 --scale 2 --theme dark --script scripts/focus.json \
  --tree "$ACTUAL/saved-corrupt.json" >/dev/null

# Save-time order, newest first: `bookmarks::toggle_saved` prepends, so the last save leads.
order=$(jq -r '[.elements[] | select((.key // "") | startswith("story-"))
    | {x: .rect[0], y: .rect[1], id: (.key | sub("story-"; "") | tonumber)}]
  | sort_by(.x, .y) | map(.id) | @csv' "$ACTUAL/saved-view.json")
if [ "$order" = "11,6,2,1" ]; then
  pass "saved view order (save-time, newest first)"
else
  bad "saved view order (got $order, want 11,6,2,1)"
fi

# Every row there is saved, so the dot would be noise: the Saved view carries none.
dots=$(jq '[.elements[] | select((.key // "") | startswith("saved-dot-"))] | length' "$ACTUAL/saved-view.json")
if [ "$dots" = 0 ]; then
  pass "saved view has no saved dots"
else
  bad "saved view has $dots saved dots"
fi

# Hover reveals `unsave` on the row itself, not only in the footer hint.
unsave=$(jq '[.elements[] | select(.text == "unsave") | select((.path // "") | contains("story-11"))] | length' "$ACTUAL/saved-view.json")
if [ "$unsave" -ge 1 ]; then
  pass "saved view hover reveals unsave"
else
  bad "saved view hover does not reveal unsave"
fi

# A corrupt file is a banner with Reload, not a silently empty list.
banner=$(jq -r '[.elements[] | select((.key // "") == "saved-banner") | .role][0] // ""' "$ACTUAL/saved-corrupt.json")
reload=$(jq -r '[.elements[] | select((.key // "") == "saved-reload") | .label][0] // ""' "$ACTUAL/saved-corrupt.json")
if [ "$banner" = "alert" ] && [ "$reload" = "Reload" ]; then
  pass "corrupt file banners with Reload"
else
  bad "corrupt file banner (role '$banner', button '$reload')"
fi

step "reduced motion (HN_REDUCED_MOTION=1)"
HN_REDUCED_MOTION=1 "$BIN" --scene resize --size 1600x900 --scale 2 --theme dark \
  --script scripts/resize.json --tree "$ACTUAL/reduced.json" --trace "$ACTUAL/reduced.trace.jsonl" >/dev/null
if [ "$(jq -r '.reduced_motion' "$ACTUAL/reduced.json")" = true ]; then
  pass "app calls cx.reduced_motion(true)"
else
  bad "app did not report reduced motion"
fi
spatial=$(jq -s --arg spatial "$SPATIAL" \
  '[.[] | select(.quality.animating == true) | (.tk // [])[] | select(.[2] == 1)
    | select((.[1] | ascii_downcase) as $p | ($spatial | split(",") | index($p)) != null)] | length' \
  "$ACTUAL/reduced.trace.jsonl")
if [ "$spatial" = 0 ]; then
  pass "reduced motion (0 active spatial tracks)"
else
  bad "reduced motion ($spatial active spatial tracks)"
fi

step "resize proof (window width, not the inset card)"
for mark in wide narrow; do
  "$BIN" --scene resize --size 1600x900 --scale 2 --theme dark --script scripts/resize.json \
    --at "mark:$mark" --tree "$ACTUAL/resize-$mark.json" >/dev/null
  jq -S . "$EXPECTED/resize-proof-$mark.json" >"$ACTUAL/resize-proof-$mark.expected.canon.json"
  jq -S . "$ACTUAL/resize-$mark.json" >"$ACTUAL/resize-proof-$mark.actual.canon.json"
  if diff -q "$ACTUAL/resize-proof-$mark.expected.canon.json" "$ACTUAL/resize-proof-$mark.actual.canon.json" >/dev/null; then
    pass "resize proof $mark tree"
  else
    bad "resize proof $mark tree"
    diff -u "$ACTUAL/resize-proof-$mark.expected.canon.json" "$ACTUAL/resize-proof-$mark.actual.canon.json" | head -40 || true
  fi
done
wide_cols=$(jq '[.elements[] | select((.key // "") | startswith("story-")) | .rect[0]] | unique | length' "$ACTUAL/resize-wide.json")
narrow_cols=$(jq '[.elements[] | select((.key // "") | startswith("story-")) | .rect[0]] | unique | length' "$ACTUAL/resize-narrow.json")
wide_win=$(jq -c '[.elements[] | select((.key // "") == "window") | .rect][0]' "$ACTUAL/resize-wide.json")
narrow_win=$(jq -c '[.elements[] | select((.key // "") == "window") | .rect][0]' "$ACTUAL/resize-narrow.json")
if [ "$wide_cols" = 2 ] && [ "$narrow_cols" = 1 ]; then
  pass "resize reflow (2 columns at $wide_win, 1 at $narrow_win)"
else
  bad "resize reflow ($wide_cols columns at $wide_win, $narrow_cols at $narrow_win)"
fi

step "story split (article left, summary pinned, discussion below)"
# Read from the trees, not the pixels: a golden cannot ratify a summary card that scrolled away
# with the discussion, a citation chip that is not a control, or a stepper left over from the
# feed. The three scenes are the same split in three states.
for name in story citation summary-generating; do
  tree="$ACTUAL/$name.json"
  q() { jq -r "$1" "$tree"; }

  # The article is the main pane's content, at the 66ch measure DESIGN.md asks for.
  title=$(q '[.elements[] | select((.key // "") == "article-title") | .text][0] // ""')
  measure=$(q '[.elements[] | select((.key // "") | test("^b[0-9]+$")) | .rect[2]] | max // 0')
  if [ "$title" = "Libghostty 1.0" ] && [ "$measure" = "448.8" ]; then
    pass "$name article (title, 66ch measure ${measure}pt)"
  else
    bad "$name article (title '$title', measure $measure)"
  fi

  # The grip is the split's resize handle, and the feed's stepper is gone.
  grip=$(q '[.elements[] | select((.key // "") == "inspector-grip") | .role][0] // ""')
  stepper=$(q '[.elements[] | select((.key // "") == "page-stepper")] | length')
  if [ "$grip" = "separator" ] && [ "$stepper" = 0 ]; then
    pass "$name grip (separator), no page stepper"
  else
    bad "$name grip '$grip', $stepper page steppers"
  fi
done

# The card and the discussion, in the two scenes that show them: the citation scene gives the
# inspector to the source viewer instead.
for name in story summary-generating; do
  tree="$ACTUAL/$name.json"
  q() { jq -r "$1" "$tree"; }

  # The summary card is pinned 12 points into the inspector and above every comment: the
  # discussion scrolls under it.
  pinned=$(q '[.elements[] | select((.key // "") == "inspector-pane") | .rect[1]][0] as $top
    | [.elements[] | select((.key // "") == "summary-card") | .rect[1]][0] as $card
    | [.elements[] | select((.key // "") | test("^comment-[0-9]+$")) | .rect[1]] | min as $first
    | "\($card == $top + 12)/\($card < $first)"')
  if [ "$pinned" = "true/true" ]; then
    pass "$name summary pinned above the discussion"
  else
    bad "$name summary pinned (card at top+12 / above first comment = '$pinned')"
  fi

  # The discussion is a flat list of top-level comments: no comment inside another.
  flat=$(q '[.elements[] | select((.path // "") | test("comment-[0-9]+/.*comment-[0-9]+"))] | length')
  count=$(q '[.elements[] | select((.key // "") == "discussion-count") | .text][0] // ""')
  if [ "$flat" = 0 ] && [ "$count" = "5 top-level comments" ]; then
    pass "$name discussion (flat, $count)"
  else
    bad "$name discussion (nested $flat, count '$count')"
  fi
done

# The done state: numbered citation chips, one per cited source, each a control.
chips=$(jq -r '[.elements[] | select((.key // "") | test("^cite-[0-9]+$")) | "\(.key):\(.role)"] | sort | join(",")' "$ACTUAL/story.json")
if [ "$chips" = "cite-901:button,cite-902:button,cite-903:button,cite-904:button,cite-905:button" ]; then
  pass "story citation chips (5, numbered, buttons)"
else
  bad "story citation chips (got '$chips')"
fi
provenance=$(jq -r '[.elements[] | select((.key // "") == "summary-provenance") | .text][0] // ""' "$ACTUAL/story.json")
if [ "$provenance" = "3 of 3 sources" ]; then
  pass "story summary provenance ($provenance)"
else
  bad "story summary provenance (got '$provenance')"
fi

# The citation scene: the source viewer takes the inspector and the article stays as context.
viewer=$(jq -r '[.elements[] | select((.key // "") == "source-viewer") | .label][0] // ""' "$ACTUAL/citation.json")
excerpt=$(jq '[.elements[] | select((.key // "") == "source-excerpt-text") | .text] | length' "$ACTUAL/citation.json")
dimmed=$(jq '[.elements[] | select((.key // "") == "article-pane") | .opacity][0] // 1' "$ACTUAL/citation.json")
if [ "$viewer" = "Citation 2" ] && [ "$excerpt" = 1 ] && [ "$dimmed" = "0.550000011920929" ]; then
  pass "citation viewer ($viewer, excerpt shown, article dimmed to $dimmed)"
else
  bad "citation viewer (label '$viewer', $excerpt excerpts, article opacity $dimmed)"
fi

# Generating: skeleton rows and the capture count, and no chips until the summary lands.
skeletons=$(jq '[.elements[] | select((.key // "") | test("^skeleton-[0-9]+$"))] | length' "$ACTUAL/summary-generating.json")
chips=$(jq '[.elements[] | select((.key // "") | test("^cite-[0-9]+$"))] | length' "$ACTUAL/summary-generating.json")
captured=$(jq -r '[.elements[] | select((.key // "") == "summary-provenance") | .text][0] // ""' "$ACTUAL/summary-generating.json")
if [ "$skeletons" = 5 ] && [ "$chips" = 0 ] && [ "$captured" = "2 of 3 sources captured" ]; then
  pass "summary generating (5 skeleton rows, '$captured', no chips)"
else
  bad "summary generating ($skeletons skeletons, $chips chips, '$captured')"
fi

step "split morph (Enter on a feed row opens the inspector from 0 width)"
"$BIN" --scene feed --size 1180x760 --scale 2 --theme dark --script scripts/open-story.json \
  --at mark:morph --tree "$ACTUAL/morph.json" >/dev/null
"$BIN" --scene feed --size 1180x760 --scale 2 --theme dark --script scripts/open-story.json \
  --at mark:open --tree "$ACTUAL/open.json" >/dev/null
morph_w=$(jq '[.elements[] | select((.key // "") == "inspector-pane") | .rect[2]][0] // 0' "$ACTUAL/morph.json")
open_w=$(jq '[.elements[] | select((.key // "") == "inspector-pane") | .rect[2]][0] // 0' "$ACTUAL/open.json")
if awk -v m="$morph_w" -v o="$open_w" 'BEGIN { exit !(m > 0 && m < o) }'; then
  pass "split morph (inspector ${morph_w}pt at mark:morph, ${open_w}pt at mark:open)"
else
  bad "split morph (inspector ${morph_w}pt at mark:morph, ${open_w}pt at mark:open)"
fi
# And what opened is the story split, not the feed behind a wide pane.
opened=$(jq -r '[.elements[] | select((.key // "") | test("^(article-pane|summary-card|comment-901)$")) | .key] | sort | join(",")' "$ACTUAL/open.json")
if [ "$opened" = "article-pane,comment-901,summary-card" ]; then
  pass "Enter opens the split (article, summary card, discussion)"
else
  bad "Enter opens the split (got '$opened')"
fi

step "sticky summary (the discussion scrolls under the card)"
# A shorter window, so the inspector's content overflows and the wheel has somewhere to go.
for mark in top scrolled; do
  "$BIN" --scene story --size 1180x560 --scale 2 --theme dark --script scripts/story-scroll.json \
    --at "mark:$mark" --tree "$ACTUAL/sticky-$mark.json" >/dev/null
done
card_top=$(jq '[.elements[] | select((.key // "") == "summary-card") | .rect[1]][0] // -1' "$ACTUAL/sticky-top.json")
card_after=$(jq '[.elements[] | select((.key // "") == "summary-card") | .rect[1]][0] // -2' "$ACTUAL/sticky-scrolled.json")
comment_top=$(jq '[.elements[] | select((.key // "") == "comment-901") | .rect[1]][0] // -1' "$ACTUAL/sticky-top.json")
comment_after=$(jq '[.elements[] | select((.key // "") == "comment-901") | .rect[1]][0] // -2' "$ACTUAL/sticky-scrolled.json")
if [ "$card_top" = "$card_after" ] && awk -v a="$comment_top" -v b="$comment_after" 'BEGIN { exit !(b < a) }'; then
  pass "sticky summary (card y=$card_top unchanged, comment-901 $comment_top -> $comment_after)"
else
  bad "sticky summary (card $card_top -> $card_after, comment-901 $comment_top -> $comment_after)"
fi

printf '\n'
if [ "$fail" = 0 ]; then
  echo "PASS: goldens, motion lint and fidelity invariants"
else
  echo "FAIL: see the lines above"
fi
exit "$fail"
