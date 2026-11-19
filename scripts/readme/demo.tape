# README demo GIF: summary, a citation and its replies, then a thread folded and reopened.
# Run through scripts/readme-screenshots.sh, which supplies the key and throwaway settings.
Output docs/screenshots/demo.gif

Set Shell "bash"
Set FontFamily "JetBrains Mono"
Set FontSize 15
Set Width 1280
Set Height 860
Set Padding 20
Set Framerate 24
Set TypingSpeed 0
Set CursorBlink false
Set Theme { "name": "hn-dark", "background": "#141719", "foreground": "#e1e5e5", "cursor": "#e1e5e5", "selection": "#25322f", "black": "#141719", "red": "#ee8a8a", "green": "#8fd1ae", "yellow": "#f3b77b", "blue": "#80c9d5", "purple": "#c3a6e0", "cyan": "#80c9d5", "white": "#e1e5e5", "brightBlack": "#353d40", "brightRed": "#ee8a8a", "brightGreen": "#8fd1ae", "brightYellow": "#f3b77b", "brightBlue": "#80c9d5", "brightPurple": "#c3a6e0", "brightCyan": "#80c9d5", "brightWhite": "#ffffff" }

# Summarise the first story off camera, so the demo opens it with its summary ready.
Hide
Type "clear && HN_THEME=dark ./target/release/hn" Enter
Wait+Screen@20s /points/
Type "c"
Wait+Screen@90s /✓ summary/
Escape
Sleep 500ms
Show

Sleep 1.2s
Type "j"
Sleep 500ms
Type "k"
Sleep 700ms
Type "c"
Sleep 2.2s
# A citation, then the comment behind it with its replies.
Tab
Sleep 1.2s
Enter
Sleep 3s
Type "j"
Sleep 1s
Type "j"
Sleep 1.5s
Escape
Sleep 1.2s
# Back in the discussion with the summary hidden: expand a thread, fold it, reopen it instantly.
Type "S"
Sleep 1.2s
Type "j"
Sleep 1s
Enter
Sleep 2.5s
Enter
Sleep 1.2s
Enter
Sleep 2s

Hide
Type "q"
