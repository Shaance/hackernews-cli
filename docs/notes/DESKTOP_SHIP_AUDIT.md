# hn-desktop ship audit: motion and polish

2026-11-17. Scope: the `desktop/` crate (hn-desktop) at `d359091`. HEAD moved to `6cfa8e0` during the audit; that commit adds a release workflow and README text and touches no source. Judged against
`mockups/desktop/DESIGN.md` and `desktop/README.md`. Read-only on source. Renders were made with the
release binary from `desktop/` (`--scene … --script … --snap/--tree/--trace/--frames/--strip`) and live
under `/private/tmp/claude-501/-Users-shaance-Documents-projects-hackernews-cli/af4da90a-a965-44d2-807e-748adfe0ccc3/scratchpad/audit/`
(abbreviated `$A` below). Headless runs always take the extracted-Markdown fallback, so anything about the
live WKWebView is marked "needs live check".

The prior audits in this folder (`VISUAL_AUDIT.md`, `SUMMARY_UX_AUDIT.md`, `DESIGN-DIRECTION.md`) are about the
terminal app and the browser preview wrapper, not this crate, so none of their findings are repeated here.

Severity: **ship-blocker** (would make a reader call it unfinished on first use), **should-fix**
(noticeable rough edge on a common path), **nice-to-have**. "Rendered" means confirmed in a frame or tree;
"code" means inferred from source.

## Findings

### M1. Opening and closing a story double-exposes the feed under the story for ~150 ms
- Severity: ship-blocker. Lens: motion. Screen: feed to story split and back. Verified: rendered (fallback
  pane); the live-page part needs live check.
- Evidence: `$A/open-morph.png` (50 ms after `Enter`), `$A/open-strip.png` (every 50 ms from 0.14 s). Tree
  samples at 16/50/100/150 ms after `Enter`: `feed-list` opacity 0.78/0.47/0.19/0.06 while `story-main` and
  `inspector` are 0.15/0.40/0.70/0.86, all in overlapping rects. Closing with `Esc` is the mirror
  (`$A/close-mid.png`, `$A/close.json`: `feed-list` 0.15/0.40/0.70 against `story-main` 0.78/0.47/0.19). The
  toolbar shows the story's "Saved" button drawn over the "Top" tab mid-transition. Cause: the tree at
  `--at mark:morph` (`$A/open-morph.json`) marks as exiting the unkeyed column `feed_body` returns
  (`main-pane/#0`, `desktop/src/app.rs:1617`, holding `feed-list` and `feed-footer`) and the toolbar's
  `feed-tabs` (`app.rs:1577`). Neither sets a transition, so each takes Rune's default `Transition::opacity()`
  (0.2 s enter, 0.15 s exit, `rune/src/el/motion.rs:193`), as does the incoming `story-main` (`app.rs:1607`).
  The crate's own gate already forbids painted outgoing reading text (`no_reading_ghosts`,
  `desktop/scripts/check_polish.py`), but only under `/inspector-pane/`, so this case escapes it.
- What carries into the real window: the inspector, the toolbar and the article header (plus "Loading
  article…") still fade in over the fading feed rows, so the double exposure is real. What differs live: the
  WKWebView is a native child that cannot take Rune opacity, so on open the page pops in at commit, and on
  close `hide_live` (`app.rs:586`) removes it instantly while Rune is still fading. Needs live check.
- What the user experiences: the most frequent transition in the app starts and ends with overlapping text,
  which reads as a glitch rather than a transition.
- Fix: give the `feed_body` column, `story-main` and the two toolbar leading groups `Transition::asymmetric(
  Transition::opacity(), Transition::none())` with a 0.15 s insertion, the pattern this crate already uses for
  page arrivals (`app.rs:1657`) and reading text (`story_view.rs:311`). The outgoing view disappears at once;
  the incoming one fades in, and the inspector keeps its slide. Widen `no_reading_ghosts` to the main pane
  and toolbar so the gate catches a regression. Size S.
- Note: the inspector does not grow its width as DESIGN.md's `MORPH` row says; it slides in at its final
  543 pt width while fading (x 1090 → 637 over ~250 ms, ~5 pt overshoot at 400 ms). That is the better choice
  (the article measure never reflows mid-motion); update DESIGN.md, not the code.

Status: fixed in run A — Main pane and toolbar cut outgoing content immediately; the inspector retains its slide.

### M2. Held `j`/`k` leaves a trail because the highlight lags the cursor
- Severity: should-fix (proposed spec change). Lens: motion. Screen: feed; the discussion uses the same
  pattern. Verified: rendered (feed), code (discussion).
- Evidence: this Mac's `KeyRepeat` is 2 (30 ms). Script `$A/hold-j.json`, 8 × `j` at 30 ms, sampled midway
  between repeats (`$A/hold-j-mid.png`): rows 7, 8, 9 carry selection fills at 10%, 34% and 52% alpha. The
  cursor row is only half lit and the row it just left is still a third lit. 100 ms after the last press it has
  settled (`$A/hold-j-after100.png`). Source: `story_row` springs its fill with `Spring::new(0.12, 0.0)`
  (`desktop/src/app.rs:1712,1758`); comment rows use the same 0.12 s spring (`story_view.rs:895`).
- Decision of record: the 120 ms colour response is written into DESIGN.md:327 and the README, so this
  is a proposal to change the spec, not a regression.
- What the user experiences: with a held key the selection reads as a two- to three-row smear, and the
  current row is never at full strength until the key is released. Feed and discussion scrolling are the
  most frequent actions in the app.
- Fix: keyboard-driven selection changes land instantly (`Spring::NONE`); keep the 0.12 s response for
  pointer hover selection, where it softens jitter. Record the source of the last selection change in `App`
  (`keyboard_navigation`, `app.rs:201`, already runs on every key) and choose `.motion()` from it. Size S.

Status: fixed in run A — Feed and discussion keyboard selection land immediately; pointer selection eases.

### P1. The help sheet cuts its last row at the default window and says "close" three times
- Severity: should-fix. Lens: polish. Screen: help (`?`). Verified: rendered.
- Evidence: `$A/help-1180-dark.png`. Tree `$A/help-1180-dark.json`: `help-scroll` is 631 pt tall,
  `help-columns` 634.75 pt, so "Esc ? Close this sheet" is sliced through its glyphs and only a 3.75 pt
  scroll reveals it. At 1180×900 and 1600×900 it fits; at 1040×700 64 pt scroll, cued only by a 10 pt edge
  fade (`desktop/src/help.rs:53`). Meanwhile the sheet says how to close itself in the last Window row, in the
  footer ("Esc closes this sheet", `help.rs:29-44`) and with the × button.
- What the user experiences: at the app's own default size the reference sheet looks broken at the bottom.
- Fix: delete `help-footer` (the Window group's "Close this sheet" row already carries it, and the sheet is
  generated from `keys::HELP`), which returns ~31 pt and makes 1180×760 fit with margin; keep the max-height
  maths in `help.rs:55-60` for shorter windows. At zoom 1.5 (`$A/help-z15.png`) the sheet shows 377 of 634 pt
  and the Summary and Window groups are below the fold with only the edge fade to say so; a visible scroll bar
  (`--scroll-bars` shows Rune can draw one) or a third column at wide sizes would help. Size S.

Status: fixed in run F — Removed the redundant footer; all 50 shortcut rows fit at the default size and remain scrollable in short windows.

### P2. Help lists two different keys as the same cap "G"
- Severity: should-fix. Lens: polish. Screen: help. Verified: rendered + code.
- Evidence: `$A/help-1180-dark.png`, Feed group: "G First story" and "G Last story". `keys.rs:305` is
  `["g"]`, `keys.rs:310` is `["G"]`; `Keybind` uppercases every cap, so the shift is lost. The Discussion group
  writes the same key as `["⇧G"]` (`keys.rs:396`) and renders correctly.
- What the user experiences: an apparent duplicate binding; the reader cannot learn Last story from the sheet.
- Fix: `keys: &["⇧G"]` for Last story (the `"⇧G"` cap already parses, `keys.rs:836`). Size S.

Status: fixed in run F — The feed help marks Last story as ⇧G.

### P3. Kit controls draw AppKit-blue focus rings; the design says amber everywhere
- Severity: should-fix. Lens: polish. Screen: help, settings, any `rune_kit` button. Verified: rendered.
- Evidence: `$A/help-1180-dark.png` (after two `Tab`s the dialog's × has a blue ring) versus
  `$A/settings-1180-light.png` (the key field's ring is amber). The app sets the view-wide default
  `cx.focus_ring(2.0, 1.0, accent)` (`desktop/src/app.rs:1453`), but rune-kit's buttons set their own ring
  from `k.c.border_focus` (`rune-kit/src/actions.rs:126,143`), and an explicit ring beats the default.
  README's VoiceOver checklist requires "amber, not AppKit blue".
- What the user experiences: two focus colours in one window; the blue one looks like a stock widget.
- Fix: build the `Kit` with `border_focus` set to the accent (`Kit::neutral(dark)` then override
  `k.c.border_focus = accent` in `view`, `app.rs:1448-1449`), so every kit control inherits amber. Size S.
Status: fixed in run C — The main kit uses the amber accent; the settings kit already did.

### P4. "Regenerate summary" is offered with no story open
- Severity: nice-to-have. Lens: polish. Screen: settings opened from the feed. Verified: rendered + code.
- Evidence: `$A/settings-1180-light.png`; `desktop/src/settings.rs:284-287` always builds the button, and it
  is focusable (tree: role button, focusable true).
- What the user experiences: a button that, from the feed, has nothing to regenerate.
- Fix: build it only when a story is open (the dialog already takes `open`/context from `app.rs`), or label it
  for the open story ("Regenerate this story's summary"). Size S.
Status: fixed in run C — Regenerate appears only when settings has an open story.

### P5. At zoom 1.5 (and at 1040 pt) the article pane is narrower than the inspector
- Severity: should-fix. Lens: polish. Screen: story split under zoom / narrow windows. Verified: rendered
  (fallback pane); live page needs live check.
- Evidence: `$A/story-zin.png` / `$A/story-zin.json` (1180×760 window, four `⌘=` to 1.5): `story-main` is
  235 pt wide, `inspector` 543 pt; the fallback banner is hard-clipped mid-word ("showing ext") with no
  ellipsis and the title metadata wraps. At zoom 1.0 in a 1040 pt window: article 489 pt vs inspector 543 pt
  (`$A/story-1040-light.json`). The inspector width is clamped in view points to 360..640
  (`desktop/src/app.rs:71-73,775-776`), so zoom shrinks only the article.
- What the user experiences: zooming in to read makes the article (the thing being read) smaller while the summary keeps its
  full width. The live page at ~350 window px likely switches to its narrow responsive layout (needs live
  check).
- Fix: cap the inspector at a fraction of the split, e.g. `min(stored, 0.45 × window width in view points)`,
  computed in `view` from the `window` rect already read at `app.rs:1456` and passed via `PaneState::range`;
  keep the persisted width untouched so `⌘0` restores it. Give the banner `.truncate()`. Size S–M.

Status: fixed in run G — Inspector width stays in window points across zoom and grip drags; the fallback banner truncates. At 1040 pt and zoom 1.0 the default inspector is still wider than the article; the grip can resize it.

### M3. Crossing the 1200 pt breakpoint jumps twice and drops the selected story
- Severity: should-fix. Lens: motion + polish. Screen: feed, resize or zoom across 1200 pt.
  Verified: rendered (tree samples).
- Evidence, the two-step jump: with `scripts/resize.json`, at 0.45 s the feed is two columns of 14; at
  0.50 s all 28 rows are stacked in one 1162 pt column (`story-16` at y 926, off screen); at 0.517 s the page
  is trimmed to 13 rows and re-centred (`story-1` y 56 → 72). Two layout jumps in two frames.
- Evidence, the lost selection: `$A/resize-right.json` on `feed-wide` (Down, Right, then shrink 1600 → 1100).
  Before: rank 16 "A barbarian tribe's mythic origin story…" is selected (`$A/resize-right-wide.png`). After:
  rank 13 "Need for Speed Underground 2…" is selected (`$A/resize-right-narrow.png`); rank 16 moved to the
  next screenful. DESIGN.md says the selection "keeps its story, not its row" and that shrinking "never drops a
  page it already has".
- The zoom half is inferred: zoom changes the view-point width the same way (`$A/feed-zout.png` is two columns
  at 1180 pt and zoom 0.75), but a zoom crossing was not sampled frame by frame.
- What the user experiences: shrinking or zooming the window silently changes which story `Enter` opens, and
  the list flickers through an intermediate one-column-of-28 frame.
- Fix: when the measured grid shrinks, re-page so the selected story stays on the visible page (pick the
  screenful that contains it) instead of clamping the index; compute columns and rows in the same frame so the
  intermediate 28-row frame never paints. No spring during live resize (layout that lags the pointer feels
  broken). For a zoom step only, a 0.15 s fade-in of the new `feed-page-{revision}` group is enough. Update
  DESIGN.md's `REFLOW` row to say resize snaps. Size M.

Status: fixed in run B — Resize and zoom keep the selected story; Saved may clip it for one frame when width and height shrink.

### P6. The first row's top inset varies with window height
- Severity: nice-to-have. Lens: polish. Screen: feed. Verified: rendered (`$A/feed-zin.png`: ~45 pt empty
  band above row 1 at zoom 1.5; 8 pt gutter elsewhere).
- Evidence: a page is one screenful of rows (11 at 1180×760, 6 at zoom 1.5, 13 at 1100×900), and
  `feed_gutter` splits the leftover height above and below them (`desktop/src/app.rs:1694-1698`). DESIGN.md's
  responsive table still says pages are 30 stories; that table is stale.
- What the user experiences: the list's top edge moves when the window height changes, and at some heights the
  first row floats well below the toolbar.
- Fix: keep the fixed `FEED_GUTTER` at the top and put the leftover below the last row; update DESIGN.md's
  page-size column. Size S.

Status: fixed in run F — Feed and loading rows use the fixed top gutter; the design describes screenful paging.

### P7. `M` does nothing on the feed, though the README and help imply it opens settings
- Severity: should-fix. Lens: polish (keyboard journey). Screen: feed. Verified: rendered (tree after `M` on
  `feed` has no `settings-*` element; the same key on `story` opens the dialog) + code.
- Evidence: `M` is bound only as a story key (`desktop/src/keys.rs:187,744`); the feed routes only `⌘,`
  (`keys.rs:230`). README: "`M` (or `⌘,`) opens the summary settings" in the general key list. The help sheet
  lists `⇧M Summary settings` under Summary with no scope.
- What the user experiences: a documented key silently does nothing on the first screen.
- Fix: add `M` to the feed's action table (it already opens the same dialog as `⌘,`), or scope the README line
  and the help row to the story. Binding it is simpler. Size S.

Status: fixed in run F — Plain uppercase M opens summary settings on the feed as an explicitly documented desktop exception.

### P8. Amber-on-grey chips are 1.6:1 contrast in light mode (continue thread, Reload, Retry, …)
- Severity: should-fix. Lens: polish (light/dark parity). Screen: comments/thread at the depth cap.
  Verified: rendered and measured.
- Evidence: `$A/chip.png` (from `$A/sg-light-2x.png`): amber text `#de7b0e` on `#bcbcc0` measures 1.6:1;
  the same chip in dark (`$A/sg-dark-2x.png`) is `#f48d2f` on `#262728`, 6.2:1. The chip fills with
  `t.control_off` (`desktop/src/story_view.rs:964`), the switch's off-track grey, not a surface token.
- The same hand-rolled chip (amber `LABEL` text on `t.control_off`) is used for Saved's "Reload"
  (`app.rs:1988`, visible in `$A/feed-saved-corrupt-light.png`), the feed's "Retry" (`app.rs:2031`) and the
  summary setup action (`story_view.rs:548-551`), so they share the light-mode problem (Reload/Retry rendered
  for Reload only; the others are inferred from the shared tokens).
- What the user experiences: in light mode the controls that recover from errors or lead deeper into a thread
  are barely legible and look disabled.
- Fix: these are three or more concrete uses of one control, so one `chip(label, on_press)` helper is
  justified; fill it with a surface token (`k.c.layer_01`/`k.c.hover`) or an accent tint
  (`accent.alpha(0.12)`), keep accent text, and check ≥4.5:1 in both themes. Or use rune-kit's
  `Button::variant(ButtonVariant::GhostMuted)` that settings already uses. Size S.
Status: fixed in run C — Quiet kit buttons use legible neutral text on both light and dark surfaces.

### M4. The save toast vanishes in one frame, and the saved dot has no `POP`
- Severity: nice-to-have. Lens: motion. Screen: feed, `s`. Verified: rendered (tree samples with
  `$A/toast.json` and `$A/save3.json`).
- Evidence: `toast-saved` fades in (opacity 0.15 → 0.94 over ~200 ms) but is 1.0 at 2.50 s and gone at
  2.55 s. When `s.toast` clears, the whole `ToastRegion` leaves the overlay list (`desktop/src/app.rs:1534-1545`),
  so nothing is left to animate out. The saved dot only fades its fill over ~120 ms (`saved-dot-3` alpha
  0 → 0x85 → 0xbc → 0xfb) with scale constant at 1.0; DESIGN.md's "Save → `POP`" (0.25 s / 0.15, 0.6 → 1.0) is
  not implemented (`saved_dot`, `app.rs:1826-1834`, has no motion).
- What the user experiences: the toast blinks away rather than leaving; the confirmation on the row itself is
  subtle enough to miss at the edge of vision.
- Fix: give the toast an exit (`Transition::opacity()` on the toast with a 0.15 s exit, kept mounted by Rune's
  exit pass, or keep `ToastRegion` mounted and toggle its child). For the dot, a one-shot `.scale(0.6 → 1.0)`
  insertion transition with `Spring::new(0.25, 0.15)`, none under reduced motion. Size S.

Status: fixed in run A — The toast fades out and the saved dot pops once, with no pop under reduced motion.

### P9. The toast straddles the footer divider
- Severity: nice-to-have. Lens: polish. Screen: feed. Verified: rendered (`$A/toast-crop.png`).
- Evidence: `toast-saved` occupies y 668–712; the footer's hairline is at y 709 and `feed-footer` starts at 710.
  The toast's lower 3 pt sits across the line, just above the page stepper.
- Fix: lift the `ToastRegion` by the footer height (or bottom-inset it from the footer's top) so it floats
  wholly above the divider. Also, "Removed from Saved" carries the amber saved dot as its lead; drop the dot on
  removal so the mark keeps meaning "saved". Size S.

Status: fixed in run A — The toast clears the measured footer and removal notices omit the saved dot.

### P10. A corrupt saved file says "No saved stories yet" and shows a raw parser error
- Severity: should-fix. Lens: polish (error state). Screen: Saved tab with an unreadable `saved.json`.
  Verified: rendered (`$A/feed-saved-corrupt-light.png`).
- Evidence: the banner reads "Saved stories could not be read · expected value at line 1 column 2" with only
  "Reload"; directly below, the empty-state copy "No saved stories yet" (`desktop/src/app.rs:1669`) contradicts
  it. DESIGN.md screen 09 promises `Reload` and `Reveal in Finder`; Reveal is absent. The footer still offers
  move/column/unsave/page hints with nothing to act on. In the toolbar the file path runs into the HN badge with
  no gap.
- What the user experiences: the app appears to have lost their saved list and speaks serde to them.
- Fix: when `s.saved.error()` is set, suppress the empty-state line; reword to "Your saved list could not be
  read. Nothing was changed." with the parser detail in secondary text; add "Reveal in Finder" (`open -R` on
  the path, macOS); hide row-only footer hints when the list is empty; give the toolbar trailing group an 8 pt
  gap. The empty (non-corrupt) state should say how to save ("Press S on a story to save it"). Size S.

Status: fixed in run F — Saved errors distinguish read and write failures, offer recovery, and no longer masquerade as an empty list.

### P11. Live-page seams that headless renders cannot show (needs live check)
- Severity: should-fix until checked. Lens: motion + polish. Screen: story split with the WKWebView.
  Verified: code only; every item needs a live check with `scripts/verify-live.sh` plus a screen recording.
- (a) Dialogs blank the article. Opening settings or help sets `want = false` and calls `setHidden`
  (`desktop/src/app.rs:571-578`), so the page vanishes in one frame while the Rune scrim fades in over ~200 ms;
  the reader sees the article region flash to the app background behind the dialog, then come back on close.
  Fix: snapshot the page (`WKWebView.takeSnapshot`) into a Rune image under the scrim while hidden, or keep the
  page visible and inset the dialog so it does not overlap the article rect. Size M.
- (b) Open and close pop. The page appears at navigation commit with no fade and disappears instantly on `Esc`
  (`hide_live`, `app.rs:586`) while Rune is still fading the rest (M1). Fix: fade the native view's
  `alphaValue` with an `NSAnimationContext` 0.15 s on show; hide immediately on close is acceptable if M1
  makes the Rune side instant too. Size S.
- (c) Child bounds during `⌘\` and grip drags follow `El::on_layout` (README), one frame behind Rune's spring;
  check for a visible gap or overlap at the pane edge during the 0.3 s `MORPH`. Size S to check.
- (d) Anything Rune draws over the article rect sits under the native view: the link chooser popover
  (`Shift+L`), tooltips and context menus anchored near the pane edge. Check that none can extend left of the
  inspector edge. Size S to check.
- (e) With the page focused, the footer still advertises `J K move`, `Tab citation`, `R refresh comments`,
  none of which reach Rune (README: "the page owns its own keyboard while it has focus"; only `Esc` still goes
  Back). Fix: when the web view is first responder, dim the row-level hints, keep "Esc back to feed", and add
  "click outside the page for shortcuts". Size S–M (needs a first-responder query from `webview.rs`).

### P12. Most hand-rolled controls have no hover or press state, and retry looks different in four places
- Severity: should-fix. Lens: polish. Screen: story split (summary card, discussion header, comments, footer).
  Verified: rendered (pixel diff of the control's box with the pointer over it vs away, `--script` `move`,
  scratch `hover.py`) + code.
- Evidence: no pixel change on hover for the summary's "Hide", the discussion's "Focus on", a comment's
  disclosure chevron, or the footer's "browser ↗" (`$A/h-hide.png`, `$A/h-focus.png`, `$A/h-disc.png`,
  `$A/h-browser.png` vs `$A/hb.png`). The footer Back button (`story_view.rs:1129`) and citation links
  (`story_view.rs:453`) do respond. Only three `.hover(` calls exist in `app.rs` and `story_view.rs`
  together. Retry has four looks: a filled accent button "Retry" (`story_view.rs:530-540`), plain text
  "Retry summaries" in `t.text` with no fill (`story_view.rs:362-363`), amber text "Retry discussion"
  (`story_view.rs:666-676`), and the grey chip "Retry replies" (`story_view.rs:982-990`).
- What the user experiences: pointer users cannot tell what is clickable in the inspector; the same recovery
  action looks like four different kinds of thing.
- Fix: route text-buttons through rune-kit `Button` (`ButtonVariant::GhostMuted` for quiet actions,
  `Submit` for the one primary retry), which brings hover, press, cursor and the focus ring (see P3) for
  free; that also replaces the P8 chips. Size M (mechanical, many call sites).
Status: fixed in run C — Kit buttons unify recovery actions with pointer cursors; compact disclosures gain hover and press feedback.

### K1. There is no app icon, so the Dock, Finder and ⌘-Tab show the generic one
- Severity: ship-blocker. Lens: packaging. Screen: the app itself. Verified: code
  (`desktop/scripts/package-macos.sh:40-41` omits `CFBundleIconFile`; no icon asset exists under `desktop/`).
- What the user experiences: the first thing anyone sees of a "finished" Mac app is a blank document icon.
- Fix: add an `AppIcon.icns` (or an asset-catalog `Assets.car` for the macOS 26 layered icon) under
  `desktop/`, copy it into `Contents/Resources/`, and set `CFBundleIconFile`/`CFBundleIconName`. Size S
  (once artwork exists).

### K2. Version 0.1.0, the README banner and unverified signing still say experimental
- Severity: ship-blocker (the label is the goal). Lens: packaging. Verified: code.
- Evidence: `desktop/Cargo.toml:3` is `0.1.0`, and `package-macos.sh:19,52-53` copies it into both
  `CFBundleShortVersionString` and `CFBundleVersion`; the release workflow stages through that script and does
  not override either key (`.github/workflows/desktop-release.yml` reads the version only to match the tag,
  lines 58-60), so build numbers never increase between builds of one version. `desktop/README.md` still opens
  with the "Highly experimental" banner. Signing and notarization exist in that workflow (commit `6cfa8e0`,
  landed during this audit) but run only when the `MACOS_CERTIFICATE_*` and `APPLE_API_KEY_*` secrets are set;
  whether they are could not be checked from here. The README lists the icon and Intel support as still
  missing.
- Fix: choose the first non-experimental version (e.g. `1.0.0`) and tag it; set `CFBundleVersion` from the CI
  run number; remove the banner; confirm one tagged release yields a stapled, notarized zip that passes
  `spctl -a -vv HN.app` on a second Mac. Size S.

### K3. Zoom, settings and the inspector have no menu-bar items
- Severity: should-fix. Lens: packaging/polish. Verified: code (no `menu_item`/`set_menu` call anywhere in
  `desktop/src`). What the menu bar actually holds needs a live check: Rune's standard menus
  (`rune/src/native_menu/default_macos.rs`) appear to be opt-in, so the bar may hold only the app menu.
- What the user experiences: a Mac user looks in the menu bar for Settings…, View ▸ Zoom In/Out/Actual Size, or
  View ▸ Show Inspector and finds nothing, so ⌘=, ⌘-, ⌘0, ⌘\ and ⌘, are discoverable only through `?`.
  There is also no visible feedback of the current zoom level after ⌘=/⌘- (`tx.instant()`, `app.rs:1103`).
- Fix: register `rune::MenuItem`s (`native_menu/mod.rs:52-83`, `.menu("View")`, `.key_equivalent(...)`)
  that call the same `App` methods the key handler calls; show a short "125%" toast on zoom change. Size S–M.
Status: fixed in run E — Native menu actions share window shortcuts and zoom changes show a transient percentage.

## Found in the live pass (2026-11-18)

Real app, real config, real Anthropic summary key, 1344×900 window. Evidence in the session scratchpad
(`cap/`), not committed.

### N1. A narrow window squeezes the article to ~150 pt
- Severity: should-fix. Verified: live at 700 pt (the Mac's only display was 700×1338 pt) and headless
  (`--scene story --size 700x1100`: article 149 pt, inspector 543 pt). Predates this session: the inspector
  range was a fixed 360–640 pt; P5's fix only scaled it with zoom.
- Decision (user): the article keeps at least half. Inspector range max = min(640, ½ window), min = min(360, max),
  applied to the pane's range so the saved width survives narrowing.
- Status: fixed — the view feeds the measured window width into the pane range; golden `story-narrow`.

### N2. The live page takes keyboard focus on open, with no click
- Severity: must-fix. Verified: live. Opening "Quad9's withdrawal from France" (blog.cloudflare.com) left the
  `WKWebView` first responder: `Tab` moved focus inside the page (cookie banner "Reject All", then "Skip to
  content"), and `Enter` pressed the page's button instead of reading citation [1]. The footer still said
  "Tab citation". The Harvard story (complexdiscovery.com) kept focus in Rune, so it depends on the page.
  `Esc` still returned to the feed (the child's local monitor intercepts it before dispatch).
- Worse, the focus outlives the story: after closing it, the hidden page stayed first responder, so in the
  feed `Return` no longer opened a story and `Esc` no longer closed the settings dialog (menu shortcuts still
  worked). A clean instance closes settings on `Esc`.
- Contract broken: DESIGN.md "The page owns its own keyboard while it has focus. Clicking into the page gives
  the `WKWebView` first responder" — only a click may hand it focus.
- Fix: a key arriving while the page holds focus without a click in it goes to Rune instead, and hiding the
  page hands focus back.
- Status: fixed and checked live on the same Cloudflare story: `Tab` selects citation [1], `Enter` opens it;
  after closing, `Return` opens a story and `Esc` closes settings.

### N4. The app menu says "hn-desktop", not "HN"
- Severity: polish. Verified: live (accessibility tree). "About hn-desktop", "Hide hn-desktop",
  "Quit hn-desktop" use the executable name; the bundle is "HN". View, Window and Help menus are right.
- Status: fixed — the bundle's executable is named after the bundle (`package-macos.sh`).

### N5. A selected citation is nearly invisible in the story view
- Severity: polish, follow-up. Verified: live stills before and after `Tab` differ only in the `[1]` marker's
  brightness. The reader cannot tell which point the citation belongs to until `Enter` opens it.
- Status: fixed in `b9460cc` — the selected chip inverts to the accent and its point gets an accent wash and
  left rule; golden `citation-selected`.

### N6. The Desktop workflow's Linux job has never passed (CI only)
- Severity: follow-up; the macOS release does not depend on it. Verified: all 11 runs since 2026-11-16 failed.
  First it could not link (`-lxkbcommon`, fixed in `cc0635b`). It now builds and tests, then fails to render:
  "no GPU adapter", so the lavapipe Vulkan driver at `VK_ICD_FILENAMES` is not loading on `ubuntu-24.04`.
  Also a Linux-only unused-import warning (`crate::keys::WindowKey`, `src/app/window.rs:24`).
- Also fixed in `cc0635b`: GitHub retired the `macos-14` image on 2026-11-02, which had cancelled every macOS
  job before it got a runner; both workflows now use `macos-15`.
- Status: fixed in `f695e35` and `009083d`. The job pins the manifest the runner installs
  (`/usr/share/vulkan/icd.d/lvp_icd.json`) and renders on `llvmpipe`; the unused import is macOS-guarded.
  First green Linux run on 2026-11-18.

### N3. Article extraction returns navigation only on some sites (shared `hn_lib`)
- Severity: follow-up (not desktop-specific). Verified: live. blog.cloudflare.com gave the summarizer "only the
  title and site navigation"; github.com/jquery/jquery/releases gave GitHub's "Sorry, something went wrong"
  page. The summary says so honestly. Extraction is `readability` in `src/reader/sources.rs:55`, shared with
  the TUI.
- Status: fixed in `6641bef` — `hidden` elements are dropped before readability, and a result under 200
  characters retries on the page's single `<main>`. Fixtures for both sites in `tests/fixtures/reader/`.

### N7. Going back to the feed and leaving a citation have no transition (user report)
- Severity: should-fix. Reported by the user after N2: `Esc` and the mouse back button snap to the feed, and
  leaving a citation snaps back to the summary, which is then outlined in the focus ring.
- Status: fixed in `2ecc4b9` (the feed arrives 24 pt from the left over a 0.22 s spring on back, both from
  keys and from the native back event) and `dc8bddc` (leaving a citation animates; the summary scroll drops
  its focus ring while a citation is focused). Checked headless with trace samples, then live over a loaded
  page on 2026-11-18: the page view drops first, then the feed slides in and fades up over ~190 ms; leaving a
  citation fades the discussion back in.

### N8. A summary point reading "placeholder [1]" (user report, unresolved)
- Seen once by the user. No code, prompt or schema produces the string, and none of the 13 responses kept in
  `~/.local/state/hn/openrouter/` contains it, so the most likely source is the model's own output.
- Logging does not depend on how the app is launched: production builds use `Network::default()`, which
  writes every provider response to `$HOME/.local/state/hn/openrouter/` (`src/reader/network.rs:34`). The last
  record is 10:14 on 2026-11-18. A story already summarized in the same session is served from memory and
  sends no request, so a repeat view leaves no record.
- Next time it appears, note the story title and time; the matching record shows whether the model wrote it.
- Since `866d4e9` new records go to `~/.local/state/hn/summary-responses/`; the old folder keeps the earlier ones.
- Seen a second time on 2026-11-19 in a README render. That render kept its records in a throwaway directory
  that the script deleted, so the reply is lost again.
- Status: guarded, cause still unconfirmed. A discussion whose overview or any point is a single word now fails
  with "Discussion summary came back incomplete. M → Enter Retry" instead of being displayed
  (`src/reader/network.rs`). In the 13 logged replies, the shortest point is 18 words and the shortest
  overview 20, so the rule rejects none of them. `scripts/readme-screenshots.sh` now copies its replies into
  `~/.local/state/hn/summary-responses/` before cleanup, so the next occurrence leaves a record.

### N9. Saved summaries: review findings deferred (`aca07ee`)
A read-only review of the persistence change found seven issues; four were fixed before merge (placeholder
discussion saved, saved line cleared by the other half, Regenerate with no key or twice, stale temp file).
These three were deferred:
- Two app processes saving different halves of the same story at the same moment can lose one half, and
  launch-time pruning can race a fresh write. `desktop.json` and `summary.json` have the same read-modify-write
  shape; no single-instance case produces it in practice. Fix with a per-story lock if a second instance ever
  becomes normal.
- Regenerating when the page extraction comes back empty replaces a restored article summary in memory with
  "No article text to summarize." (not saved, so a restart restores the good one). Same behaviour as before
  persistence existed.
- Closing a story mid-request and reopening it from a refreshed feed saves the newer comment count against the
  older capture, so the next "N new comments" is too small. Fix by carrying the count with the request, as
  provenance is.
Not exercised live: Regenerate's file update, a real "N new comments" delta, and the 30-day prune.

## Where the code differs from DESIGN.md's motion spec

| Spec item | In code | Verdict |
| --- | --- | --- |
| Open story, `MORPH` width 0 → fraction | Inspector slides in at full width with a fade (0.30/0.16) | Keep the code; fix the main-pane crossfade (M1); update DESIGN.md |
| Save, `POP` + toast | Dot fades colour 120 ms; toast fades in, snaps out | M4 |
| Fold, `REFLOW` 0.40 | Replies leave at once and later siblings glide up in ~100 ms; on unfold replies fade in over ~150 ms while siblings glide down (tree samples, `$A/fold.json`; `story_view.rs:817-821`, commit `df3aa34`) | Deliberate and better for a frequent action; update DESIGN.md |
| 1 ↔ 2 columns, `REFLOW` 0.40 | Snaps, in two steps | M3 |
| Settings/help/link chooser, `MORPH` | Help and settings fade ~200 ms in, ~150 ms out, no scale (tree samples, `$A/help-open.json`, `$A/set.json`); link chooser not sampled | Fine; update DESIGN.md |
| Summary arrival, `growth` + `GLIDE` | Code only: each section's result rises 8 pt from 0.4 opacity on a 0.20 s spring when it lands (`story_view.rs:352-394`) | Looks right; not sampled |
| Page/tab arrival 24 pt + fade 220 ms | As specified (tree samples, `$A/tab.json`: `feed-page-1` x 26 → 8, opacity 0.39 → 0.99) | OK |
| Row selection 120 ms colour | As specified | Proposed change for keyboard (M2) |
| Reduced motion | Slides removed; story open/close become a 150 ms crossfade, which still double-exposes (feed 0.71 against story 0.28 at 0.2 s) | Fixed by M1 |
| Interruptibility | `Enter` then `Esc` 60 ms later then `Enter` retargets smoothly from the current values; no restart or jump | OK |

## Not verified

- Anything drawn by the live WKWebView (P11 a–e, M1's live half, P5's page layout): needs `verify-live.sh`
  plus a screen recording.
- Focus mode dimming, trail segment `SNAP`, re-root `GLIDE`, the link chooser, summary arrival: not sampled
  (summary arrival needs the async `AppSpec` path the tests near `desktop/src/app.rs:4151-4191` use).
- Summary failed and auth-failed states: no scene exists, so they were read from code only (P12).
- Whether the CI signing secrets are configured (K2).
- VoiceOver: the README's manual checklist has never been run; it remains an open gate.

## Ship list (must land before dropping "experimental")

1. M1: remove the feed/story double exposure on open and close (S).
2. K1: app icon (S, once artwork exists).
3. K2: version, build number, README banner, one verified notarized release (S).
4. M3: keep the selected story through resize and zoom, with no intermediate frame (M).
5. P3 + P12 + P8: one button style with hover, press, amber focus and legible light-mode contrast (M).
6. P1 + P2 + P7: help sheet fits at the default size, shows `⇧G`, and `M` works where documented (S).
7. P10: corrupt Saved state stops claiming the list is empty (S).
8. P11: run the live-page checks; fix (a) the dialog blanking if it reproduces (M).
9. P5: cap the inspector so zooming in does not shrink the article (S–M).

Should follow soon after: M2, K3, M4, P9, P4, P6.

## Do not do

- Animate the feed grid during a live window resize. Layout that lags the pointer feels broken. Snap it and
  fix the selection instead (M3).
- Grow the inspector's width from 0 as DESIGN.md says. It reflows the article measure on every frame of the
  open. The current fixed-width slide is calmer.
- Add motion to `j`/`k`, Page Up/Down or `n`/`p` beyond what exists. These are high-frequency keyboard
  actions; the right direction is less (M2), not a scroll glide or a row slide.
- Animate zoom (`⌘=`/`⌘-`). Browser zoom is an instant change everywhere, and `tx.instant()` is correct.
- Add a scale "pop" to dialogs. The opacity fade is enough for a modal summoned by a key, and a scale on a
  720 pt sheet with dense text looks soft mid-flight.
- Stagger the feed rows on arrival. The 24 pt group arrival already orients; per-row stagger would delay
  reading the first rows.
- Slow the comment fold to the spec's 0.40 s. Folding is frequent; 0.12–0.15 s is already right.
