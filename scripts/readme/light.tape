# README still of the reader in the light palette, cut from light.gif by scripts/readme-screenshots.sh.
Output docs/screenshots/light.gif

Set Shell "bash"
Set FontFamily "JetBrains Mono"
Set FontSize 15
Set Width 1280
Set Height 780
Set Padding 20
Set TypingSpeed 0
Set CursorBlink false
Set Theme { "name": "hn-light", "background": "#faf9f5", "foreground": "#24292c", "cursor": "#24292c", "selection": "#e9eeec", "black": "#24292c", "red": "#a33a3a", "green": "#2f6b4f", "yellow": "#995015", "blue": "#006b79", "purple": "#6b4a8a", "cyan": "#006b79", "white": "#faf9f5", "brightBlack": "#d4d9d8", "brightRed": "#a33a3a", "brightGreen": "#2f6b4f", "brightYellow": "#995015", "brightBlue": "#006b79", "brightPurple": "#6b4a8a", "brightCyan": "#006b79", "brightWhite": "#ffffff" }

Hide
Type "clear && HN_THEME=light ./target/release/hn" Enter
Wait+Screen@20s /points/
Type "jj"
Type "c"
Wait+Screen@60s /✓ summary/
Sleep 1s
Show
Sleep 1.5s
Hide
Type "q"
