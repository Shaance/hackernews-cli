#!/usr/bin/env bash
# The headless golden gate for hn-desktop (docs/notes/DESKTOP_IMPLEMENTATION_PLAN.md v4, "Acceptance gate").
#
# For every frozen scenario it renders the app offscreen, then checks four independent things:
#
#   1. the element tree, canonicalised with `jq -S`, matches expected/ — byte-identical on
#      macOS, semantic-only elsewhere (see SEMANTIC below);
#   2. the PNG is within 0.5% of expected/ (macOS only: cross-platform pixel equality is not
#      claimed, so Linux and Windows run with HN_TREES_ONLY=1);
#   3. the motion lint: nothing animates at rest, and no spatial track is ever active;
#   4. fidelity invariants read from the tree, not the pixels — column x-positions, rank order,
#      stepper text, and the selected row's focus flags. These exist so a self-generated golden
#      cannot ratify a wrong layout, so their expected values come from expected/, never from
#      the tree just rendered.
#
# Usage:  bash desktop/scripts/check_goldens.sh
#   HN_TREES_ONLY=1   skip the PNG comparison (Linux/Windows CI)
#   HN_SKIP_BUILD=1   use the binary already in target/debug
#   HN_EXACT_TREES=0  force the semantic tree comparison (the non-macOS default)
#   HN_GOLDEN_ACTUAL  choose where rendered comparison artifacts are written
set -euo pipefail

cd "$(dirname "$0")/.."

EXPECTED=verification/expected
ACTUAL=${HN_GOLDEN_ACTUAL:-verification/actual}
BIN=target/debug/hn-desktop
TREES_ONLY=${HN_TREES_ONLY:-0}
# A frame is at rest once the last scripted event has settled. The resize script's last
# animation ends at 0.6 s, so one second after launch nothing may still be moving.
AT_REST_MS=1000
# Spatial properties: anything that moves or resizes. A fade is not one of them.
SPATIAL='frame,offset,scale,rotation,travel,scroll'
# Off macOS the geometry differs (font metrics, layout rounding, GPU rounding), so the tree
# comparison is semantic: keys, paths, roles, labels, text and focus flags, with no rects or
# paint values. macOS compares the trees byte-for-byte, which is the stronger claim.
SEMANTIC='del(.t, .anim_t, .animating, .time_scale, .viewport, .scale)
  | .elements |= map(del(.rect, .scale, .opacity, .blur, .fill, .color, .loopPhase))
  | .accessibility |= walk(if type == "object" then del(.bounds) else . end)'
if [ "${HN_EXACT_TREES:-}" = 1 ]; then
  EXACT_TREES=1
elif [ "${HN_EXACT_TREES:-}" = 0 ]; then
  EXACT_TREES=0
elif [ "$(uname -s)" = Darwin ]; then
  EXACT_TREES=1
else
  EXACT_TREES=0
fi

fail=0
pass() { printf '  ok    %s\n' "$*"; }
bad() { printf '  FAIL  %s\n' "$*"; fail=1; }
step() { printf '\n== %s\n' "$*"; }

# Canonicalise a tree for comparison: exact on macOS, semantic elsewhere.
canon() { # in out
  if [ "$EXACT_TREES" = 1 ]; then
    jq -S . "$1" >"$2"
  else
    jq -S "$SEMANTIC" "$1" >"$2"
  fi
}

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
# The story split's frozen states. They share the render/tree/pixel/motion loops with the
# feed scenes but not the feed's fidelity loop: a story has no rank order and no page stepper.
# `comments` is the discussion at the story's top level; `thread` is the same discussion re-rooted
# four levels down with focus mode on, which is the state the trail and the dimming exist for.
STORY_SCENES=(
  "story|story|1180x760|scripts/story.json"
  "citation-selected|story|1180x760|scripts/citation-selected.json"
  "story-narrow|story|700x1100|scripts/story.json"
  "comments|comments|1180x760|scripts/thread.json"
  "thread|thread|1180x760|scripts/thread.json"
  "citation|citation|1180x760|scripts/story.json"
  "summary-generating|summary-generating|1180x760|scripts/story.json"
  "settings|settings|1180x760|scripts/focus.json"
  "settings-story|settings-story|1180x760|scripts/focus.json"
  "help|help|1180x760|scripts/focus.json"
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
if [ "$EXACT_TREES" = 1 ]; then
  printf '        exact trees (macOS)\n'
else
  printf '        semantic trees (no geometry: %s)\n' "$(uname -s)"
fi
for row in "${ALL[@]}"; do
  IFS='|' read -r name _ _ _ _ <<<"$row"
  canon "$EXPECTED/$name.json" "$ACTUAL/$name.expected.canon.json"
  canon "$ACTUAL/$name.json" "$ACTUAL/$name.actual.canon.json"
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

  # Column x-positions: one column below the 1200pt breakpoint, two at or above it.
  cols=$(jq '[.elements[] | select((.key // "") | startswith("story-")) | .rect[0]] | unique | length' "$tree")
  if [ "$cols" = "$want" ]; then
    pass "$name columns ($cols at $size)"
  else
    bad "$name columns ($cols at $size, want $want)"
  fi

  # Rank order and column-major placement. The fixture's Top ids are 1..30 in rank order, so
  # reading the story keys in (x, y) order must give 1..N: down the first column, then the next.
  # N is pinned from the golden, not counted from the tree just rendered: a self-generated
  # golden must not be able to ratify a wrong number of rows.
  order=$(jq -r '[.elements[] | select((.key // "") | startswith("story-"))
      | {x: .rect[0], y: .rect[1], id: (.key | sub("story-"; "") | tonumber)}]
    | sort_by(.x, .y) | map(.id) | @csv' "$tree")
  want_n=$(jq '[.elements[] | select((.key // "") | startswith("story-"))] | length' "$EXPECTED/$name.json")
  got_n=$(jq '[.elements[] | select((.key // "") | startswith("story-"))] | length' "$tree")
  want_order=$(seq 1 "$want_n" | paste -sd, -)
  if [ "$got_n" = "$want_n" ] && [ "$order" = "$want_order" ]; then
    pass "$name rank order (column-major 1..$want_n)"
  else
    bad "$name rank order ($got_n rows: $order, want $want_n: $want_order)"
  fi

  # The rank labels agree with the keys.
  ranks=$(jq -r '[.elements[] | select((.key // "") | startswith("rank-")) | .text | tonumber] | sort | @csv' "$tree")
  if [ "$ranks" = "$want_order" ]; then
    pass "$name rank labels (1..$want_n)"
  else
    bad "$name rank labels (got $ranks, want $want_order)"
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

# The thread's own motion: the trail snaps, the rows reflow, and a re-root glides the viewport.
# Under reduced motion the same keys must still land the same state, with nothing travelling.
HN_REDUCED_MOTION=1 "$BIN" --scene story --size 1180x760 --scale 2 --theme dark \
  --script scripts/thread-keys.json --at mark:reroot --tree "$ACTUAL/reduced-thread.json" \
  --trace "$ACTUAL/reduced-thread.trace.jsonl" >/dev/null
reduced_rows=$(jq -r '[.elements[] | select((.key // "") | test("^comment-[0-9]+$"))
    | {y: .rect[1], id: (.key | sub("comment-"; ""))}] | sort_by(.y) | map(.id) | join(",")' \
  "$ACTUAL/reduced-thread.json")
reduced_trail=$(jq -r '[.accessibility | .. | objects | select((.key // "") | startswith("trail-")) | .key] | join(",")' \
  "$ACTUAL/reduced-thread.json")
reduced_flag=$(jq -r '.reduced_motion' "$ACTUAL/reduced-thread.json")
if [ "$reduced_flag" = true ] && [ "$reduced_rows" = "901000,9010000,90100000,90100001,9010001,9010002" ] \
  && [ "$reduced_trail" = "trail-901,trail-9010,trail-90100,trail-901000" ]; then
  pass "reduced motion thread (re-root lands the same tree and trail)"
else
  bad "reduced motion thread (flag $reduced_flag, rows '$reduced_rows', trail '$reduced_trail')"
fi
spatial=$(jq -s --arg spatial "$SPATIAL" \
  '[.[] | select(.quality.animating == true) | (.tk // [])[] | select(.[2] == 1)
    | select((.[1] | ascii_downcase) as $p | ($spatial | split(",") | index($p)) != null)] | length' \
  "$ACTUAL/reduced-thread.trace.jsonl")
if [ "$spatial" = 0 ]; then
  pass "reduced motion thread (0 active spatial tracks)"
else
  bad "reduced motion thread ($spatial active spatial tracks)"
fi

# The same keys with motion on must land the same state: reduced motion changes how the window
# gets there, never where it ends up. The bounds still differ by fractions while the animated run
# settles, so the comparison is the state the reader sees — the rows, the trail, the selection,
# the dimming and the focus pill — not the pixels.
"$BIN" --scene story --size 1180x760 --scale 2 --theme dark \
  --script scripts/thread-keys.json --at mark:reroot --tree "$ACTUAL/motion-thread.json" >/dev/null
thread_state() { # tree
  jq -r '
    ([.elements[] | select((.key // "") | test("^comment-[0-9]+$"))
      | {y: .rect[1], id: (.key | sub("comment-"; ""))}] | sort_by(.y) | map(.id) | join(",")) as $rows
    | ([.accessibility | .. | objects | select((.key // "") | startswith("trail-")) | .key] | join(",")) as $trail
    | ([.elements[] | select(.itemSelected == true) | .key] | join(",")) as $sel
    | ([.elements[] | select((.key // "") | test("^comment-[0-9]+$")) | select(.opacity < 0.99)
      | .key] | sort | join(",")) as $dim
    | ([.elements[] | select((.key // "") == "discussion-focus")] | length) as $pill
    | "rows=\($rows) trail=\($trail) sel=\($sel) dim=\($dim) pill=\($pill)"' "$1"
}
reduced_state=$(thread_state "$ACTUAL/reduced-thread.json")
motion_state=$(thread_state "$ACTUAL/motion-thread.json")
if [ "$reduced_state" = "$motion_state" ]; then
  pass "reduced motion thread (same state as the animated run: $reduced_state)"
else
  bad "reduced motion thread (state differs)"
  printf '        reduced: %s\n        animated: %s\n' "$reduced_state" "$motion_state"
fi

step "resize proof (window width, not the inset card)"
for mark in wide narrow; do
  "$BIN" --scene resize --size 1600x900 --scale 2 --theme dark --script scripts/resize.json \
    --at "mark:$mark" --tree "$ACTUAL/resize-$mark.json" >/dev/null
  canon "$EXPECTED/resize-proof-$mark.json" "$ACTUAL/resize-proof-$mark.expected.canon.json"
  canon "$ACTUAL/resize-$mark.json" "$ACTUAL/resize-proof-$mark.actual.canon.json"
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

# The footer's hints fit the narrowest split: the last one ends inside the footer's own box, so a
# key added to the row cannot silently run off the edge of the window.
for name in story citation summary-generating; do
  tree="$ACTUAL/$name.json"
  edge=$(jq '[.elements[] | select((.key // "") == "story-footer") | .rect[0] + .rect[2]][0] // 0' "$tree")
  last=$(jq '[.elements[] | select((.path // "") | contains("story-footer")) | select(.text != null)
      | .rect[0] + .rect[2]] | max // 0' "$tree")
  if awk -v a="$last" -v b="$edge" 'BEGIN { exit !(a > 0 && a <= b) }'; then
    pass "$name footer hints fit (last ends at ${last}pt, footer at ${edge}pt)"
  else
    bad "$name footer hints overflow (last ends at ${last}pt, footer at ${edge}pt)"
  fi
done

# The card and the discussion, in the three scenes that show them: the citation scene gives the
# inspector to the source viewer instead.
for name in story comments summary-generating; do
  tree="$ACTUAL/$name.json"
  q() { jq -r "$1" "$tree"; }

  # The summary card aligns with the pane top and stays above every comment: the
  # discussion scrolls under it.
  pinned=$(q '[.elements[] | select((.key // "") == "inspector-pane") | .rect[1]][0] as $top
    | [.elements[] | select((.key // "") == "summary-card") | .rect[1]][0] as $card
    | [.elements[] | select((.key // "") | test("^comment-[0-9]+$")) | .rect[1]] | min as $first
    | "\($card == $top)/\($card < $first)"')
  if [ "$pinned" = "true/true" ]; then
    pass "$name summary pinned above the discussion"
  else
    bad "$name summary pinned (card at pane top / above first comment = '$pinned')"
  fi

  # The discussion is a tree, and focus mode starts enabled.
  deep=$(q '[.elements[] | select((.key // "") | test("^comment-[0-9]+$")) | select(.level > 1)] | length')
  focus=$(q '[.elements[] | select((.key // "") == "discussion-focus") | .label][0] // ""')
  if [ "$deep" -ge 1 ] && [ "$focus" = "Turn focus mode off" ]; then
    pass "$name discussion (tree, $deep deeper rows, focus mode on)"
  else
    bad "$name discussion ($deep deeper rows, focus '$focus')"
  fi
done

step "comment tree (depth rails, the cap, collapsed counts)"
# Read from the tree, not the pixels: the rows in draw order with the depth each one draws at, the
# hairline per ancestor level, and the cap's label. A golden cannot ratify a tree that indents
# every reply by the same amount, or a `continue thread` that counts the wrong replies.
rows=$(jq -r '[.elements[] | select((.key // "") | test("^comment-[0-9]+$"))
    | {y: .rect[1], id: (.key | sub("comment-"; "")), level: .level}]
  | sort_by(.y) | map("\(.id)@\(.level)") | join(",")' "$ACTUAL/comments.json")
want_rows="901@1,9010@2,90100@3,901000@4,90101@3,9011@2,902@1,903@1,9030@2,90300@3,904@1,905@1"
if [ "$rows" = "$want_rows" ]; then
  pass "comments rows (12, depth first, levels 1-4)"
else
  bad "comments rows (got $rows)"
fi

# One hairline per ancestor level: 901000 draws three, a top-level comment draws none.
guides=$(jq '[.elements[] | select((.key // "") | test("^guide-[0-9]+-[0-9]+$"))] | length' "$ACTUAL/comments.json")
deepest=$(jq '[.elements[] | select((.key // "") | test("^guide-901000-[0-9]+$"))] | length' "$ACTUAL/comments.json")
if [ "$guides" = 12 ] && [ "$deepest" = 3 ]; then
  pass "comments depth rails ($guides hairlines, 3 under 901000)"
else
  bad "comments depth rails ($guides hairlines, $deepest under 901000)"
fi

# The cap: 901000 draws at depth 3 with five replies, so its children are behind a label that
# counts every reply under it, not just the ones that would have been rows.
cont=$(jq -r '[.elements[] | select((.path // "") | endswith("comment-continue-901000/label")) | .text][0] // ""' "$ACTUAL/comments.json")
cont_label=$(jq -r '[.elements[] | select((.key // "") == "comment-continue-901000") | .label][0] // ""' "$ACTUAL/comments.json")
if [ "$cont" = "continue thread → 5 replies" ] && [ "$cont_label" = "Continue thread, 5 replies" ]; then
  pass "comments cap ('$cont')"
else
  bad "comments cap (text '$cont', label '$cont_label')"
fi

# At the story's top level the trail is one segment long, so it is not drawn at all.
trail=$(jq '[.elements[] | select((.key // "") == "trail")] | length' "$ACTUAL/comments.json")
if [ "$trail" = 0 ]; then
  pass "comments trail (absent at the top level)"
else
  bad "comments trail ($trail trail strips at the top level)"
fi

step "thread (sticky trail, focus mode)"
# The trail is the cursor's ancestor chain, so its last segment is the comment being read: the
# ancestors are buttons, the focused comment is not.
segments=$(jq -r '[.accessibility | .. | objects | select((.key // "") | startswith("trail-"))
    | "\(.key):\(.role)"] | join(",")' "$ACTUAL/thread.json")
want_segments="trail-901:button,trail-9010:button,trail-90100:button,trail-901000:button,trail-9010000:label"
if [ "$segments" = "$want_segments" ]; then
  pass "thread trail (5 segments, last one not pressable)"
else
  bad "thread trail (got $segments)"
fi

# The trail sits above the rows it describes, and the re-rooted comment draws at depth 0.
trail_y=$(jq '[.elements[] | select((.key // "") == "trail") | .rect[1]][0] // -1' "$ACTUAL/thread.json")
first_y=$(jq '[.elements[] | select((.key // "") == "comment-901000") | .rect[1]][0] // -2' "$ACTUAL/thread.json")
root_level=$(jq '[.elements[] | select((.key // "") == "comment-901000") | .level][0] // 0' "$ACTUAL/thread.json")
if awk -v a="$trail_y" -v b="$first_y" 'BEGIN { exit !(a >= 0 && a < b) }' && [ "$root_level" = 1 ]; then
  pass "thread re-root (trail y=$trail_y above the root row y=$first_y, drawn at level 1)"
else
  bad "thread re-root (trail y=$trail_y, root row y=$first_y at level $root_level)"
fi

# Focus marks the selected comment without dimming the remaining discussion.
focus=$(jq -r '[.elements[] | select((.key // "") == "discussion-focus") | .label][0] // ""' "$ACTUAL/thread.json")
rail=$(jq -r '[.elements[] | select((.key // "") | startswith("focus-rail-")) | .key] | join(",")' "$ACTUAL/thread.json")
dim=$(jq '[.elements[] | select((.key // "") | test("^comment-[0-9]+$")) | select(.opacity < 0.99)] | length' "$ACTUAL/thread.json")
if [ "$focus" = "Turn focus mode off" ] && [ "$rail" = "focus-rail-9010000" ] && [ "$dim" = 0 ]; then
  pass "thread focus (selected rail, all comments readable)"
else
  bad "thread focus (label '$focus', rail '$rail', $dim dimmed comments)"
fi

# The focused comment is the selected row, and it is the one the trail ends on.
selected=$(jq -r '[.elements[] | select(.itemSelected == true) | .key] | join(",")' "$ACTUAL/thread.json")
if [ "$selected" = "comment-9010000" ]; then
  pass "thread selection (comment-9010000, the last trail segment)"
else
  bad "thread selection (got '$selected')"
fi

step "sticky trail (the rows scroll under the ancestor chain)"
# The trail is the one thing in the discussion that does not move: the rows pass under it, so the
# comment being read keeps its ancestry on screen however far down the thread it is.
for mark in top scrolled; do
  "$BIN" --scene thread --size 1180x760 --scale 2 --theme dark --script scripts/thread-scroll.json \
    --at "mark:$mark" --tree "$ACTUAL/trail-$mark.json" >/dev/null
done
trail_top=$(jq '[.elements[] | select((.key // "") == "trail") | .rect[1]][0] // -1' "$ACTUAL/trail-top.json")
trail_after=$(jq '[.elements[] | select((.key // "") == "trail") | .rect[1]][0] // -2' "$ACTUAL/trail-scrolled.json")
viewport=$(jq '[.elements[] | select((.key // "") == "discussion-scroll") | .rect[1]][0] // -3' "$ACTUAL/trail-scrolled.json")
header=$(jq '[.elements[] | select((.key // "") == "discussion-header") | .rect][0] // [-1,-1,0,0]' "$ACTUAL/trail-scrolled.json")
header_y=$(jq '.[1]' <<<"$header")
header_bottom=$(jq '.[1] + .[3]' <<<"$header")
row_top=$(jq '[.elements[] | select((.key // "") == "comment-901000") | .rect[1]][0] // -1' "$ACTUAL/trail-top.json")
row_after=$(jq '[.elements[] | select((.key // "") == "comment-901000") | .rect[1]][0] // -2' "$ACTUAL/trail-scrolled.json")
if [ "$header_y" = "$viewport" ] && [ "$trail_top" = "$trail_after" ] \
  && awk -v a="$row_top" -v b="$row_after" -v t="$trail_after" -v h="$header_y" -v end="$header_bottom" \
    'BEGIN { exit !(b < a && t >= h && t < end) }'; then
  pass "sticky trail (inside pinned header y=$header_y, trail y=$trail_after, rows $row_top -> $row_after)"
else
  bad "sticky trail (trail $trail_top -> $trail_after, viewport $viewport, rows $row_top -> $row_after)"
fi

step "thread keys (j [ ] u h l f Enter ⌫ Esc L Tab S, asserted after each)"
# One render per mark in scripts/thread-keys.json, and the tree the app drew after each key. The
# script is the proof that the keys are wired to the model, not just that the model has the
# transitions: every assertion below reads the rendered tree, never the state a test set up.
for mark in start next sibling sibling-back parent folded unfolded focus reroot pop focus2 focus3 \
  links links-closed citation citation-closed summary-hidden summary-shown; do
  "$BIN" --scene story --size 1180x760 --scale 2 --theme dark --script scripts/thread-keys.json \
    --at "mark:$mark" --tree "$ACTUAL/keys-$mark.json" >/dev/null
done

# A mark's tree, read four ways: the rows in draw order, the selected row, the trail's segments,
# and the rows focus mode dimmed.
key_rows() { jq -r '[.elements[] | select((.key // "") | test("^comment-[0-9]+$"))
    | {y: .rect[1], id: (.key | sub("comment-"; ""))}] | sort_by(.y) | map(.id) | join(",")' "$ACTUAL/keys-$1.json"; }
key_sel() { jq -r '[.elements[] | select(.itemSelected == true) | .key] | join(",")' "$ACTUAL/keys-$1.json"; }
key_trail() { jq -r '[.accessibility | .. | objects | select((.key // "") | startswith("trail-")) | .key] | join(",")' "$ACTUAL/keys-$1.json"; }
key_focus() { jq -r '[.elements[] | select((.key // "") == "discussion-focus") | .label][0] // ""' "$ACTUAL/keys-$1.json"; }
key_rail() { jq -r '[.elements[] | select((.key // "") | startswith("focus-rail-")) | .key] | join(",")' "$ACTUAL/keys-$1.json"; }

check() { # mark got want what
  if [ "$2" = "$3" ]; then
    pass "keys $1: $4"
  else
    bad "keys $1: $4 (got '$2', want '$3')"
  fi
}

check start "$(key_rows start)" "901,9010,90100,901000,90101,9011,902,903,9030,90300,904,905" "12 rows, depth first"
check start "$(key_sel start)" "comment-901" "the cursor starts on the first row"
check start "$(key_trail start)" "" "no trail at the top level"
check start "$(key_rail start)" "focus-rail-901" "focus starts on the selected comment"

check next "$(key_sel next)" "comment-9010" "j moves to the next visible row"
check next "$(key_trail next)" "trail-901,trail-9010" "the trail gains a segment"

check sibling "$(key_sel sibling)" "comment-9011" "] moves to the next sibling thread"
check sibling "$(key_trail sibling)" "trail-901,trail-9011" "the trail follows the cursor"

check sibling-back "$(key_sel sibling-back)" "comment-9010" "[ moves back to the previous sibling"
check sibling-back "$(key_trail sibling-back)" "trail-901,trail-9010" "and the trail follows it back"

check parent "$(key_sel parent)" "comment-901" "u moves to the parent"
check parent "$(key_trail parent)" "" "and the trail collapses back to one segment"

check folded "$(key_rows folded)" "901,9010,9011,902,903,9030,90300,904,905" "h folds 9010's replies"
hidden=$(jq -r '[.elements[] | select((.key // "") == "comment-hidden-9010") | .label][0] // ""' "$ACTUAL/keys-folded.json")
check folded "$hidden" "Show 3 hidden replies" "the fold counts the rows it hid"

check unfolded "$(key_rows unfolded)" "901,9010,90100,901000,90101,9011,902,903,9030,90300,904,905" "l unfolds them again"

check focus "$(key_focus focus)" "Turn focus mode on" "f turns focus mode off"
check focus "$(key_rail focus)" "focus-rail-9010" "the selected comment stays marked with focus mode off"

check reroot "$(key_rows reroot)" "901000,9010000,90100000,90100001,9010001,9010002" "Enter on continue thread re-roots"
check reroot "$(key_trail reroot)" "trail-901,trail-9010,trail-90100,trail-901000" "and pushes a trail segment"
check reroot "$(key_sel reroot)" "comment-901000" "the re-rooted comment is the cursor"

check pop "$(key_rows pop)" "90100,901000,9010000,90100000,90100001,9010001,9010002" "Backspace pops one segment"
check pop "$(key_trail pop)" "trail-901,trail-9010,trail-90100" "the trail is one segment shorter"

check focus2 "$(key_focus focus2)" "Turn focus mode on" "f turns focus off deeper in the thread"
check focus2 "$(key_rail focus2)" "focus-rail-9010000" "the selected comment stays marked deeper in the thread"
check focus3 "$(key_focus focus3)" "Turn focus mode off" "f turns focus back on"
check focus3 "$(key_rail focus3)" "focus-rail-9010000" "the selected comment is marked"
check focus3 "$(key_rows focus3)" "90100,901000,9010000,90100000,90100001,9010001,9010002" "the story stays open"

chip=$(jq -r '[.elements[] | select((.key // "") == "links-chip") | .expanded][0]' "$ACTUAL/keys-links.json")
links=$(jq '[.elements[] | select((.key // "") | test("^link-[0-9]+$"))] | length' "$ACTUAL/keys-links.json")
check links "$chip/$links" "true/1" "L opens the chooser on the focused comment's links"
chip=$(jq -r '[.elements[] | select((.key // "") == "links-chip") | .expanded][0]' "$ACTUAL/keys-links-closed.json")
links=$(jq '[.elements[] | select((.key // "") | test("^link-[0-9]+$"))] | length' "$ACTUAL/keys-links-closed.json")
check links-closed "$chip/$links" "false/0" "Esc closes it and keeps the discussion"

# Tab selects a citation and Enter reads it; Esc comes back to the discussion, not to the feed.
viewer=$(jq '[.elements[] | select((.key // "") == "source-viewer")] | length' "$ACTUAL/keys-citation.json")
check citation "$viewer" "1" "Tab then Enter opens the cited comment"
viewer=$(jq '[.elements[] | select((.key // "") == "source-viewer")] | length' "$ACTUAL/keys-citation-closed.json")
check citation-closed "$viewer" "0" "Esc closes the viewer and keeps the discussion"

# S collapses the summary card to its points and back, and the discussion does not move.
hidden=$(jq '[.elements[] | select((.key // "") == "summary-folded" and .exiting != true)] | length' "$ACTUAL/keys-summary-hidden.json")
shown=$(jq '[.elements[] | select((.key // "") == "summary-open" and .exiting != true)] | length' "$ACTUAL/keys-summary-shown.json")
check summary-hidden "$hidden" "1" "S collapses the summary card"
check summary-shown "$shown" "1" "S expands it again"

# Every transition leaves the focused comment inside the discussion's viewport: the selection is
# scrolled into view, so a key never moves the cursor somewhere the reader cannot see.
for mark in start next sibling sibling-back parent folded unfolded focus reroot pop focus2 focus3 \
  links links-closed citation-closed summary-hidden summary-shown; do
  inside=$(jq -r '[.elements[] | select((.key // "") == "discussion-scroll") | .rect][0] as $vp
    | [.elements[] | select(.itemSelected == true) | select((.key // "") | test("^comment-[0-9]+$")) | .rect][0] as $row
    | if $vp == null or $row == null then "missing"
      else "\($row[1] >= $vp[1] and $row[1] + $row[3] <= $vp[1] + $vp[3])" end' "$ACTUAL/keys-$mark.json")
  check "$mark" "$inside" "true" "the focused comment is inside the viewport"
done

# The done state: numbered references are inline controls named for their source.
chips=$(jq -r '[.elements[] | select((.key // "") | test("^cite-[0-9]+$")) | "\(.key):\(.role)"] | sort | join(",")' "$ACTUAL/story.json")
inline=$(jq '[.elements[] | select((.key // "") | test("^cite-[0-9]+$")) | select((.path // "") | contains("summary-text-"))] | length' "$ACTUAL/story.json")
first_label=$(jq -r '[.elements[] | select(.key == "cite-0") | .label][0] // ""' "$ACTUAL/story.json")
if [ "$chips" = "cite-0:button,cite-1:button,cite-2:button,cite-3:button,cite-4:button" ] && [ "$inline" = 5 ] && [ "$first_label" = "Read comment by tptacek" ]; then
  pass "story inline citations (5 source-named controls)"
else
  bad "story inline citations (got '$chips', $inline inline, first '$first_label')"
fi
synopsis=$(jq -r '[.elements[] | select(.key == "article-summary") | .text][0] // ""' "$ACTUAL/story.json")
headings=$(jq -r '[.elements[] | select(.key == "article-summary-heading" or .key == "discussion-summary-heading") | .text] | join(",")' "$ACTUAL/story.json")
if [ "$synopsis" = "The article describes Libghostty and its design." ] && [ "$headings" = "Article,Discussion" ]; then
  pass "story summary (article synopsis and discussion sections)"
else
  bad "story summary (synopsis '$synopsis', sections '$headings')"
fi

# The citation scene preserves the hidden inspector and exposes the full cited comment.
viewer=$(jq -r '[.elements[] | select((.key // "") == "source-viewer") | .label][0] // ""' "$ACTUAL/citation.json")
comment=$(jq '[.elements[] | select(.key == "source-comment-text" and (.text // "") != "")] | length' "$ACTUAL/citation.json")
hidden=$(jq -r '[.elements[] | select(.key == "inspector") | (.inert == true and .opacity == 0)][0] // false' "$ACTUAL/citation.json")
article_opacity=$(jq '[.elements[] | select(.key == "article-pane") | .opacity][0] // 0' "$ACTUAL/citation.json")
if [ "$viewer" = "Comment by tptacek" ] && [ "$comment" = 1 ] && [ "$hidden" = "true" ] && [ "$article_opacity" = "1.0" ]; then
  pass "citation viewer ($viewer, full comment, retained inert inspector)"
else
  bad "citation viewer (label '$viewer', $comment comments, inspector '$hidden', article opacity $article_opacity)"
fi

# One section may be ready while the other is still generating.
skeletons=$(jq '[.elements[] | select((.key // "") | test("^skeleton-[0-9]+$"))] | length' "$ACTUAL/summary-generating.json")
chips=$(jq '[.elements[] | select((.key // "") | test("^cite-[0-9]+$"))] | length' "$ACTUAL/summary-generating.json")
status=$(jq -r '[.elements[] | select(.key == "summary-status") | .text][0] // ""' "$ACTUAL/summary-generating.json")
article=$(jq '[.elements[] | select(.key == "article-summary" and (.text // "") != "")] | length' "$ACTUAL/summary-generating.json")
if [ "$skeletons" = 2 ] && [ "$chips" = 0 ] && [ "$status" = "Reading the discussion…" ] && [ "$article" = 1 ]; then
  pass "summary generating (article ready, discussion pending)"
else
  bad "summary generating ($skeletons skeletons, $chips citations, '$status', $article article summaries)"
fi

step "settings and help (the two overlays, read from the tree)"
# The settings dialog is the terminal reader's `M` overlay. The property the whole design rests on
# is that the key field carries no text in the tree at all: a key that cannot be rendered cannot
# be logged, traced or screenshotted.
sq() { jq -r "$1" "$ACTUAL/settings.json"; }
dialog=$(sq '[.accessibility | .. | objects | select((.key // "") == "settings-dialog")
  | "\(.role)/\(.name)/\(.states | join(","))"][0] // ""')
if [ "$dialog" = "dialog/Summary settings/modal" ]; then
  pass "settings dialog (modal, named)"
else
  bad "settings dialog (got '$dialog')"
fi

field=$(sq '[.accessibility | .. | objects | select((.key // "") == "input")
  | "\(.role)/\(.name)/\(.states | join(","))"][0] // ""')
secure=$(sq '[.elements[] | select((.key // "") == "input") | .secure][0] // false')
texts=$(sq '[.elements[] | select((.key // "") == "input") | select(.text != null)] | length')
if [ "$field" = "password-input/OpenRouter key/focused" ] && [ "$secure" = true ] && [ "$texts" = 0 ]; then
  pass "settings key field (password input, named, focused, no text)"
else
  bad "settings key field (a11y '$field', secure $secure, $texts texts)"
fi

# The scene's key is a fake, and it must not appear anywhere in the tree: not in a text run, not
# in a label, not in a path.
leak=$(grep -c 'EXAMPLEKEYNOTREAL' "$ACTUAL/settings.json" || true)
if [ "$leak" = 0 ]; then
  pass "settings key never reaches the tree"
else
  bad "settings key appears $leak times in the tree"
fi

# The two choices, and both models resolved for the chosen provider.
provider=$(sq '[.accessibility | .. | objects | select(.role == "radio-group" and .name == "Provider")
  | .children[] | "\(.name):\(.states | join(","))"] | join(",")')
profile=$(sq '[.accessibility | .. | objects | select(.role == "radio-group" and .name == "Summary quality")
  | .children[] | "\(.name):\(.states | join(","))"] | join(",")')
models=$(sq '[.elements[] | select((.key // "") == "fast-model" or (.key // "") == "thorough-model") | .text] | join(",")')
duplicate_labels=$(sq '[.accessibility | .. | objects | select(.role == "label")
  | select(.name == "Provider" or .name == "Automatic summaries" or .name == "Summary quality"
      or .name == "OpenRouter key" or .name == "Anthropic key")] | length')
if [ "$provider" = "OpenRouter:checked,Anthropic:unchecked" ] \
  && [ "$profile" = "Fast:checked,Thorough:unchecked" ] \
  && [ "$duplicate_labels" = 0 ] \
  && [ "$models" = "anthropic/claude-haiku-5.5,anthropic/claude-sonnet-5.5" ]; then
  pass "settings choices ($provider; $profile; models $models)"
else
  bad "settings choices (provider '$provider', profile '$profile', models '$models', duplicate labels '$duplicate_labels')"
fi

# Feed-opened settings has the switch and common buttons, but no story action.
controls=$(sq '[.accessibility | .. | objects | select(.role == "switch" or .role == "button")
  | .name] | join(",")')
if [ "$controls" = "Close,Automatic summaries,Cancel,Apply" ]; then
  pass "settings controls ($controls)"
else
  bad "settings controls (got '$controls')"
fi

# The sheet is `keys::HELP` rendered, so the gate checks the shape rather than a hand-copied list:
# four groups in draw order, and every row a keybind plus a label.
hq() { jq -r "$1" "$ACTUAL/help.json"; }
dialog=$(hq '[.accessibility | .. | objects | select((.key // "") == "help-dialog")
  | "\(.role)/\(.name)/\(.states | join(","))"][0] // ""')
if [ "$dialog" = "dialog/Keyboard/modal" ]; then
  pass "help sheet (modal, named)"
else
  bad "help sheet (got '$dialog')"
fi

titles=$(hq '[.elements[] | select((.key // "") | startswith("help-title-")) | .text] | join(",")')
if [ "$titles" = "Feed,Summary,Discussion,Window" ]; then
  pass "help groups ($titles)"
else
  bad "help groups (got '$titles')"
fi

# A row with no keys is a label with nothing to press; a row with no label is keys with no
# meaning. The intended four groups contain 50 rows, with help grouped under Window.
rows=$(hq '[.accessibility | .. | objects | select((.key // "") | startswith("help-row-"))] | length')
want_rows=50
bare=$(hq '[.accessibility | .. | objects | select((.key // "") | startswith("help-row-"))
  | select(([.children[]? | select(.role == "group" and (.name // "") != "")] | length) != 1
        or ([.children[]? | select(.role == "label" and (.name // "") != "")] | length) != 1)] | length')
if [ "$rows" = "$want_rows" ] && [ "$bare" = 0 ]; then
  pass "help rows ($rows rows, each a keybind and a label)"
else
  bad "help rows ($rows of $want_rows, $bare without both a keybind and a label)"
fi

step "settings and help (interactive: the keys and the presses, not just the picture)"
# `M` opens the dialog from the story split and `Esc` closes it without losing the discussion.
"$BIN" --scene story --size 1180x760 --scale 2 --theme dark --script scripts/settings-keys.json \
  --at mark:open --tree "$ACTUAL/settings-open.json" >/dev/null
"$BIN" --scene story --size 1180x760 --scale 2 --theme dark --script scripts/settings-keys.json \
  --at mark:closed --tree "$ACTUAL/settings-closed.json" >/dev/null
opened=$(jq '[.elements[] | select((.key // "") == "settings-dialog")] | length' "$ACTUAL/settings-open.json")
closed=$(jq '[.elements[] | select((.key // "") == "settings-dialog")] | length' "$ACTUAL/settings-closed.json")
kept=$(jq '[.elements[] | select((.key // "") | test("^comment-[0-9]+$"))] | length' "$ACTUAL/settings-closed.json")
regenerate=$(jq '[.elements[] | select(.role == "button" and .label == "Regenerate summary" and .focusable and (.inert | not))] | length' "$ACTUAL/settings-open.json")
if [ "$opened" = 1 ] && [ "$closed" = 0 ] && [ "$kept" -ge 1 ] && [ "$regenerate" = 1 ]; then
  pass "M opens settings with Regenerate, Esc closes it and the discussion stays ($kept rows)"
else
  bad "M/Esc ($opened dialogs open, $closed after Esc, $kept comment rows, $regenerate Regenerate buttons)"
fi

# A press on Anthropic and Thorough updates both choices, the key field and the models.
"$BIN" --scene settings --size 1180x760 --scale 2 --theme dark --script scripts/settings.json \
  --at mark:picked --tree "$ACTUAL/settings-picked.json" >/dev/null
picked=$(jq -r '[.elements[] | select((.key // "") == "segment1")
  | select((.path // "") | contains("settings-providers")) | .itemSelected][0] // false' "$ACTUAL/settings-picked.json")
thorough=$(jq -r '[.elements[] | select((.key // "") == "thorough")
  | select((.path // "") | contains("settings-quality")) | .itemSelected][0] // false' "$ACTUAL/settings-picked.json")
field=$(jq -r '[.elements[] | select((.key // "") == "input") | .label][0] // ""' "$ACTUAL/settings-picked.json")
model=$(jq -r '[.elements[] | select((.key // "") == "thorough-model") | .text][0] // ""' "$ACTUAL/settings-picked.json")
if [ "$picked" = true ] && [ "$thorough" = true ] && [ "$field" = "Anthropic key" ] \
  && [ "$model" = "claude-sonnet-5-5" ]; then
  pass "settings presses (Anthropic + Thorough -> $model, field '$field')"
else
  bad "settings presses (anthropic $picked, thorough $thorough, field '$field', model '$model')"
fi

# The sheet's close button dismisses it and the feed behind comes back to life.
"$BIN" --scene help --size 1180x760 --scale 2 --theme dark --script scripts/help.json \
  --at mark:closed --tree "$ACTUAL/help-closed.json" >/dev/null
sheet=$(jq '[.elements[] | select((.key // "") == "help-dialog")] | length' "$ACTUAL/help-closed.json")
live=$(jq '[.elements[] | select((.key // "") | startswith("story-")) | select(.inert == false)] | length' "$ACTUAL/help-closed.json")
if [ "$sheet" = 0 ] && [ "$live" -ge 1 ]; then
  pass "help close (sheet gone, $live feed rows live again)"
else
  bad "help close ($sheet sheets left, $live live feed rows)"
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

step "desktop feed columns and page opacity"
for at in 0.5 0.65 0.85 1.05 1.10 1.43 1.9; do
  "$BIN" --scene feed-wide --size 1600x900 --scale 2 --theme dark \
    --script scripts/feed-navigation.json --at "$at" --tree "$ACTUAL/feed-nav-$at.json" \
    --trace "$ACTUAL/feed-nav-$at.trace.jsonl" >/dev/null
done
HN_REDUCED_MOTION=1 "$BIN" --scene feed-wide --size 1600x900 --scale 2 --theme dark \
  --script scripts/feed-navigation.json --at 1.05 --tree "$ACTUAL/feed-nav-reduced.json" >/dev/null
python3 - "$ACTUAL" <<'PYFEED'
import json, sys
from pathlib import Path
root = Path(sys.argv[1])
def elements(at):
    return json.loads((root / f"feed-nav-{at}.json").read_text())["elements"]
def selected(at):
    return [e["key"] for e in elements(at) if e.get("itemSelected") and e.get("key", "").startswith("story-")]
def group(at):
    return [e for e in elements(at) if e.get("key", "").startswith("feed-page-")]
before = [e for e in elements("0.5") if e.get("key", "").startswith("story-")]
# The same column-major arithmetic as the visible grid, derived from the page actually shown.
ordered = sorted(before, key=lambda e: (e["rect"][0], e["rect"][1]))
assert selected("0.65") == [ordered[(len(ordered) + 1) // 2]["key"]]
assert selected("0.85") == [ordered[1]["key"]]
for key in ["title-1", "title-2"]:
    assert [e["rect"] for e in elements("0.5") if e.get("key") == key] == [e["rect"] for e in elements("0.85") if e.get("key") == key]
assert group("0.5")[0]["key"] == group("0.85")[0]["key"] == "feed-page-0"
a, b = group("1.05"), group("1.10")
assert len(a) == len(b) == 1, "outgoing pages must not remain interactive"
assert 0.2 < a[0]["opacity"] < b[0]["opacity"] < 1
assert a[0]["rect"][0] > b[0]["rect"][0] > group("0.5")[0]["rect"][0] + 2, "directional motion must remain visible at 50/100 ms"
assert group("reduced")[0]["rect"][:3] == group("0.5")[0]["rect"][:3]
assert group("reduced")[0]["opacity"] == 1
assert group("1.43")[0]["key"] == "feed-page-2"
assert group("1.43")[0]["rect"][0] < group("0.5")[0]["rect"][0], "previous pages must enter from the left"
assert len(group("1.9")) == 1 and group("1.9")[0]["opacity"] == 1
assert group("1.9")[0]["key"] == "feed-page-3", "rapid next/back must converge"
assert all(e["opacity"] == a[0]["opacity"] for e in elements("1.05") if e.get("key", "").startswith("story-")), "rows must inherit the group opacity without individual fades"
for at in ["0.85", "1.10"]:
    frames = [json.loads(line) for line in (root / f"feed-nav-{at}.trace.jsonl").read_text().splitlines()]
    names = {}
    for frame in frames:
        names.update({node["id"]: node["nm"] for node in frame.get("m", [])})
        tracks = [track for track in frame.get("tk", []) if track[2] == 1]
        if 600 <= frame.get("t", -1) <= 850:
            assert not tracks, "keyboard selection must land instantly"
        if frame.get("t", -1) >= 1000:
            assert all(track[1] == "presence" for track in tracks), "page motion must use only group presence"
            assert all(names.get(track[0], str(track[0])).rsplit("/", 1)[-1].startswith("feed-page-") for track in tracks), "only the page group may transition"

PYFEED
pass "columns preserve row geometry; keyboard selection is instant; page groups move directionally; reduced motion lands instantly; rapid paging converges"

step "loading shells and actual AppSpec arrival contracts"
cargo test --locked --offline --lib render_ -- --ignored --test-threads=1
pass "loading accessibility and delayed startup/tab/page arrivals (normal and reduced motion)"

step "reading transitions and compact layouts"
python3 scripts/check_polish.py
pass "reading surfaces never overlap; reduced motion is instant; dialogs and saved rows stay aligned"

printf '\n'
if [ "$fail" = 0 ]; then
  echo "PASS: goldens, motion lint and fidelity invariants"
else
  echo "FAIL: see the lines above"
fi
exit "$fail"
