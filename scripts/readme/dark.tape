# README stills in the dark palette; the wrapper cuts them from dark.gif at fixed times.
# Run through scripts/readme-screenshots.sh, and keep its still times in step with the Sleeps here.
Output docs/screenshots/dark.gif

Set Shell "bash"
Set FontFamily "JetBrains Mono"
Set FontSize 15
Set Width 1280
Set Height 780
Set Padding 20
Set Framerate 24
Set TypingSpeed 0
Set CursorBlink false
Set Theme { "name": "hn-dark", "background": "#141719", "foreground": "#e1e5e5", "cursor": "#e1e5e5", "selection": "#25322f", "black": "#141719", "red": "#ee8a8a", "green": "#8fd1ae", "yellow": "#f3b77b", "blue": "#80c9d5", "purple": "#c3a6e0", "cyan": "#80c9d5", "white": "#e1e5e5", "brightBlack": "#353d40", "brightRed": "#ee8a8a", "brightGreen": "#8fd1ae", "brightYellow": "#f3b77b", "brightBlue": "#80c9d5", "brightPurple": "#c3a6e0", "brightCyan": "#80c9d5", "brightWhite": "#ffffff" }

# Open the third story once off camera, so its summary is ready when the demo opens it.
Hide
Type "clear && HN_THEME=dark ./target/release/hn" Enter
Wait+Screen@20s /points/
Type "jj"
Type "c"
Wait+Screen@60s /✓ summary/
Escape
Sleep 500ms
Show

Sleep 1.2s
Type "c"
Sleep 2s
Tab
Sleep 1.5s
Enter
Sleep 2s
Escape
Sleep 800ms
Type "jjj"
Sleep 1.2s
Type "?"
Sleep 1.5s
Type "?"
Sleep 800ms
Escape
Sleep 1s

Hide
Type "q"
