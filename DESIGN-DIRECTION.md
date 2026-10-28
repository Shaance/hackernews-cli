# Quiet Workbench

Selected by the user from the public design gallery on 2026-10-26; implementation authorized with “Yes let’s go”. Scope is the browser preview wrapper, not the native terminal application.

Reference: https://hn-preview-designs.vercel.app/quiet-workbench.png

Product register; preserve behavior and overhaul wrapper styling. Restrained composition, standard density, restrained color, product-led content, instant interaction. The native keyboard-driven CLI is the working surface; the original website remains an external iframe.

Experience: land directly in a working feed, press Enter for article/summary, use the existing restart action after expiry or disconnect.
Visual: compact hn. masthead, one shared two-pane workspace, warm amber actions, quiet charcoal surfaces. Let the content occupy the viewport.
Interaction: preserve startup/focus/navigation/recovery; use static visible focus and pressed states without ornamental motion.

| Token | Value |
|---|---|
| Canvas | #111214 |
| Surface | #181a1d |
| Terminal | #10141b (existing native/xterm background) |
| Ink | #ece8e0 |
| Muted | #abb2bd |
| Accent | #e6aa69 |
| Rule | #343940 |
| Type | system sans; existing terminal monospace |

Keep the actual terminal text, first-story selection, data, controls, real article and GitHub install action. No duplicated category buttons, gradients, glow, hero illustration, added tabs, UI framework or external fonts. Dark appearance only, as selected. On phones stack terminal above article and keep the install link visible.

Generated-image artifacts are not implementation requirements: shortened summary/comments, synthetic terminal line spacing, missing native settings/status labels, altered article text and small surface gradients. Use real CLI output and unmodified external page. A single truthful timestamp includes time; native CLI typography and colors stay unchanged.

Validation: desktop feed/read, mobile390/320 widths, narrow layout equivalent to desktop200% zoom, keyboard focus, captured page limit, quit/restart, blocked embedding/recovery, no horizontal wrapper overflow. Measure wrapper text contrast. Real mobile keyboard/toolbar behavior is not proven by desktop emulation.
