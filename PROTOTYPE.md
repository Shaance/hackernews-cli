# Prototype history

Current setup and behavior are documented in [README.md](README.md). Use
`./scripts/dev.sh` from this checkout. The notes below describe past experiments;
the old private Summarizer runner and its absolute paths are obsolete.

<details>
<summary>Historical experiment notes</summary>

# Disposable summary UX experiment

This worktree includes the existing local reader improvements. The original worktree remains untouched. No commit or push has been made.

Run the local experiment with:

```sh
/Users/chris.ha/.local/state/ai-skills/run-ledgers/4211726b5308/summary-prototype/try.sh
```

The runner reuses the private summarizer project's article extractor and model client. It does not start or modify the production server, use its databases, or change its authentication. It binds only to localhost and uses a fresh token for this launch. The script stops the runner when the CLI exits. No private Go dependency was added to the public Rust project.

By default the runner reads only model-provider settings from summarizer-compose/docker-compose-local.yml using Docker Compose configuration resolution; the Docker daemon does not need to run. An optional first argument supplies a development env file instead. Supported model keys are Groq, OpenAI, CLIProxy, Cerebras, and Fireworks, chosen in that order if several are present. CLIProxy also requires its service to be running. No credentials are included here.

- **Enter/o** opens the story workspace and its website. In a wide Ghostty window, the prototype asks terminal-browser for a right-hand split. **c** opens the discussion; if this session already has a browser pane, it updates that pane to the selected story.
- **o** reads the original website; **O** explicitly opens the regular browser from either the feed or reader. Other terminals, windows below 110 columns before the first split, and a missing terminal-browser use the regular browser. Use Ghostty's native pane-focus controls to return to HN. The prototype activates the `hn-article` key table only on its browser pane, making **⌘W** gracefully quit that browser. Other panes keep their normal close action. Reload Ghostty configuration once with **⌘⇧,**, then open a new article pane. If activation fails, the launch notice asks you to reload Ghostty and press o; it does not claim the shortcut is active. **Ctrl+Q** remains the browser's own quit shortcut.
- The CLI keeps the story, one inline AI summary, and the live comment tree. The website lives in the companion pane. There are no Article/Discussion tabs or extracted Markdown article view.
- **S** requests both summaries and opens a pinned panel above the comments. Results appear there as they arrive. The panel fits its full contents when there is room, growing up to two thirds of the reader body so comments remain visible. Short results give unused space back to comments; there is no compact preview or extra expansion step. S shows/hides it without changing the discussion position. The status row remains visible when it is hidden.
- **j/k, arrows, and Page Up/Down** always navigate the discussion beneath the summary. **J/K** scroll the summary independently when it overflows; the panel boundary shows that hint and its line range.
- **Tab/Shift-Tab** selects and reveals a cited summary point; **Enter** opens its captured comment with parent context. **Esc** returns to the pinned panel and unchanged discussion position, then to the feed. Existing reply expansion, **s** save, and **r** retry remain.


The local launcher adds the signature-verified terminal-browser v0.13.4 installation to PATH for this run. Each launch uses its own OS-assigned localhost port. No global browser installation or macOS security bypass is required. One companion pane is retained per running CLI session; a newly opened story replaces the owned article tab. Feed cursor movement does not navigate the browser. Following a website link does not change which HN story owns the summary.

Native Ghostty split rendering, focus and pane reuse still need a live check: computer-use access to Ghostty was denied. The browser executable's signature and version were verified. The live non-Ghostty terminal flow passed with real HN comments and model summaries: opt-in generation, collapsed arrival, source/back at 40×12, 80×24 and 120×40, replies expanding from 10 to 12 visible comments and collapsing to 10, external-browser fallback, retained results on reopen, and clean exit. Captures are in `/private/tmp/hn-workspace-live/`. These checks do not establish that the native split works.

Latest sticky-summary validation: all 62 Rust tests, formatting, and build passed. A live HN/model run on an isolated localhost port verified immediate expansion on the first S, completion without moving the comment viewport, visible readiness, and exact comment-position restoration after viewing the summary and a citation at 40×12, 80×24, and 120×40. j/k crossed the summary/comment boundary both ways; replies still expanded from 10 to 12 visible comments and collapsed to 10. The terminal exited cleanly. Captures are in `/private/tmp/hn-sticky-live/`. This supersedes the earlier collapsed-arrival behavior described in the preceding validation history.

Prototype limits: session-only results; up to 60 comment fetches in breadth-first order, using up to 1,800 characters per comment for the model. Full collected comment text stays readable in source view. Article model input remains capped at 24,000 extracted characters. The bridge returns a separate partial-source flag, displayed as “Partial source · opening portion only” above the article summary. It no longer appends a percentage sentence to generated prose. This cutoff still needs revisiting with cleaner extraction and a real input budget. Ask HN shows the original post and can summarize it on demand. Extracted article text is used only as model input. Source view is a snapshot; Discussion is the expandable live tree. No provider settings, persistent jobs, regeneration action, or billing UI.

The localhost runner produces one concise article summary, without the normal service's glossary/FAQ pipeline. It exercises real extraction and generation, but is not a deployment test of that service. The original Rust worktree and both private repositories remain unchanged.

Try a short article, a long article, a busy discussion, Ask HN, and an inaccessible article. Check whether reading, revealing summaries, and inspecting sources feel like one continuous journey.

## Earlier summary experiments

These are engineering trials of the real TUI, not user research or a general model benchmark. The model request still asks for a 50–70 word article overview and at most three distinct discussion points. Repetitive discussion overviews are hidden when points exist. Extraction errors use readable wording. Self-posts now use the Article section.

| Observation | Implication for the eventual design |
| --- | --- |
| Three real article outputs were 56, 61, and 62 words (including coverage labels). The Discussion heading fit on the first 80×24 screen in all five story cases. | A concise output contract matters more than adding tabs. This informed concise output; the latest reader embeds each summary above its own content. |
| Source selection, source reading, and return worked across five stories at 40×12, 80×24, and 120×40. | Keep source IDs and the collected source snapshot in the CLI. Selection should reveal its point; leaving a source should restore the summary position. |
| A Reuters article failed extraction while discussion completed. Reopening made no new requests in controlled request-count checks. | Article and discussion need independent state, owned by the story rather than the visible screen. |
| One generated point misread “half a gig” of font files as price. Another cited an opposing reply as supporting evidence. | A real comment ID does not establish that the claim is supported. Quality evaluation must compare claims with source text. |
| A tighter grounding prompt removed the first price error in one run, but introduced an abbreviation error and still overstated some claims. | Prompt revisions alone did not establish reliable grounding. Preserve failing examples as an evaluation set. |
| On the same five captured discussions, the configured Groq path took 1.25–2.98 seconds; the configured OpenAI path took 26.12–35.50 seconds and still made the price error. | Do not equate slower with better. Keep the current fast default for this prototype; compare model quality and latency together before choosing a final default. |
| OpenAI first returned no usable text because its reasoning consumed the 1,500-token budget; 6,000 allowed the comparison to complete. | Provider integration needs model-specific request settings; an endpoint and key alone do not define behavior. |
| Literal first-paragraph excerpts from the selected comments often showed praise or setup and omitted the substantive detail later in the comment. | Do not replace summaries with naive excerpts. A future preview would need a relevant passage, not just the first characters. |
| The Sound article used only its first 48% and said so. Busy HN input is also bounded and explicitly marked partial. | Source coverage is a product decision. Thread identity alone is insufficient to describe what a result summarizes. |

The source-excerpt comparison was a display-only replay of real captured comments and source selections; it is not another mode in the shipped prototype. Layout replay timings are not model latency measurements.

Still unresolved: whether you personally prefer article-first or discussion-first; whether a good cited comment should open in the expandable live thread; how much discussion to collect; and which model/settings meet an acceptable grounding standard. No production provider architecture, persistence, automatic regeneration, or verification model was added.

Validation evidence and captured inputs live in the local experiment directory next to the launcher: `iteration2-live/`, `grounding-check/`, `grounding-openai/`, `iteration2-final-ui/`, and `excerpt-trial/`. Full source snapshots used for cited-comment reading remain in memory in the TUI; the experiment records contain only the excerpts actually sent to the model. Generated claims have not been exhaustively fact-checked.

## Browser-pane Cmd+W trial

The existing Ghostty config was preserved byte-for-byte and one inactive table definition was appended:

```ini
keybind = hn-article/cmd+w=text:\x11
```

The prototype obtains the pane ID from its owned browser socket and performs `activate_key_table:hn-article` on that terminal only. It does not reload config, activate a table on HN/other terminals, or change close-confirmation settings. Closing through the window controls remains Ghostty's normal behavior.

Validation: combined config accepted by Ghostty; exact activation AppleScript compiled against the installed dictionary; Rust build and 62 tests passed. Native Cmd+W and the adjacent-pane comparison remain blocked because computer-use access to Ghostty is denied. Do not treat these checks as proof of native behavior. Pending check: reload config, launch a fresh article pane, press Cmd+W there and verify only it closes without a prompt; then use a disposable non-browser pane with a running process to verify its usual confirmation still appears and cancel it. Also verify reopening an article creates a working companion pane.

The pre-change Ghostty config backup is `/private/tmp/hn-close-research/ghostty-config-before`. To undo only this trial, remove the `hn-article/cmd+w` line and its HN comment, reload Ghostty, and restart any existing article pane.

### Browser exited but pane remained open

The user's live screenshot confirmed that Cmd+W quits the browser but leaves Ghostty's “Process exited” screen. Ghostty v1.3.1 `src/apprt/embedded.zig` sets `wait-after-command = true` whenever the scripting API supplies a custom command; the false wait flag cannot override it. terminal-browser v0.13.4 supplies both, so the earlier assumption that its false flag would close the pane was wrong.

The prototype-local CLI bundle now supplies initial input (`GScI`) containing ` exec ` plus the already shell-quoted browser launch command and a newline, instead of supplying a custom command (`GScC`). Ghostty starts its normal shell; exec replaces that shell with the existing browser wrapper. When the browser quits, the wrapper exits and Ghostty can close the pane normally. No global exit/confirmation settings or other panes were changed. The Cmd+W table remains scoped to the owned browser pane.

Patch: `~/.local/state/ai-skills/run-ledgers/4211726b5308/summary-prototype/browser-trial/ghostty-exit.patch`. It applies only to that experiment's `native-extract/terminal-browser/cli/dist/main.js`; the signed Electron app is unchanged. An upstream browser replacement would need this patch reapplied or the underlying Ghostty behavior fixed.

Validation: the modified CLI bundle and embedded automation script both parse. An isolated zsh PTY ran the actual terminal-browser `--version` through the new initial-input/exec command, printed v0.13.4, and exited with status zero without returning to a prompt. This checks shell lifecycle, not native Ghostty closure. Native confirmation-free closure and the adjacent-pane comparison remain unverified because computer-use access is denied. Dismiss the already-exited pane with any key, then open a fresh article pane to try the adjustment. No config reload or Rust rebuild is needed for this launcher-only change.

### Quiet browser startup (supersedes the initial-input workaround)

The user's next screenshot showed the injected command, login banner, and interactive shell prompt during startup. The local browser patch now restores Ghostty's direct custom-command launch. Its noninteractive wrapper clears the screen before starting the browser. No command is typed into an interactive shell.

For a Ghostty browser launched with the existing split metadata, the CLI captures its own pane ID before opening its session. A successful process exit closes that exact pane through Ghostty's AppleScript close command, which bypasses confirmation for that target. Failed launches remain visible. Ordinary invocations, non-Ghostty terminals, and invocations whose parent TTY equals their own do not register this cleanup. Cmd+W still sends the browser's graceful quit shortcut; cleanup follows its exit. No global config or other pane is changed.

The cumulative patch remains `browser-trial/ghostty-exit.patch` under the local experiment directory. Validation: the bundle and embedded script parse; the close script compiles without execution; isolated ownership checks cover the dedicated split, parent pane, ordinary invocation, another terminal, and failure exit. The exact wrapper ran the real browser --version in a PTY, with clear-screen bytes first, no echoed launch command or prompt, and exit code zero. These are not native Ghostty visual/close tests. Native access remains denied; opening a fresh article pane and closing it with Cmd+W is still the live acceptance check. A very brief macOS login banner before the wrapper runs has not been visually ruled out. No reload or Rust rebuild is needed.

### Continue reading after a summary visit

Previously only a second S restored the saved position; j/Down through the summary selected comment zero, and Page Down continued from the top. Both paths now consume the saved return position at the visible summary boundary. Source views retain their own scrolling. The existing hide action uses the same restore path.

Validation: a new regression reproduced comment zero before the fix. The fix passes 63 tests and build/format checks. The live TUI used real HN comments and model summaries at 40×12, 80×24, and 120×40. At every size, j, Down, and Page Down through a summary restored the exact previous comment viewport. The live test also caught and eliminated a two-line transition through the top discussion header. Reply expansion/navigation/collapse remained 10 → 12 → 10, and exit restored the terminal. Evidence: `/private/tmp/hn-summary-resume-live/`; runner: `/private/tmp/hn-summary-resume-check.py`. This was a PTY keyboard test; Ghostty trackpad input was not directly exercised.

### Pinned summary (supersedes the temporary summary visit)

The user chose a summary that stays pinned above independently scrolling comments. Summary state now owns its own scroll offset. S no longer changes comment cursor/scroll or stores a return bookmark. The previous summary-to-discussion scrolling handoff has been removed. Source selection scrolls only the pinned panel, and source/back keeps the discussion in place. At the end of the discussion, enlarging the viewport by hiding the summary preserves the current top line instead of jumping backward to fill the extra room.

On very short terminals the bounded panel shows only a few lines; J/K keeps its full contents reachable. This is still a keyboard prototype; Ghostty trackpad behavior requires native validation.

Pinned validation: all 61 current tests, build, formatting, and diff checks passed. The live PTY used real HN comments and model summaries at 40×12, 80×24, and 120×40. It verified unchanged discussion on opening and result arrival; pinned summary during comment selection and paging; independent J/K summary scroll; source/back; hiding/reopening without changing the comment anchor; reply navigation/expansion/collapse (10 → 12 → 10); and clean exit with terminal restoration. Captures: `/private/tmp/hn-pinned-live/`. Runner: `/private/tmp/hn-pinned-check.py`. Superseded summary-traversal tests were replaced with pinned-panel checks.

### Full pinned summary sized to content

The compact-preview proposal was rejected because pressing S already expresses intent to read the summary. The prototype now sizes the complete panel to its content, with a two-thirds-of-body ceiling only when the result would otherwise crowd out comments. This replaces the fixed 14-row ceiling. The title uses its actual wrapped height, the footer uses two rows, and blank rows after summary section labels and coverage were removed. Paragraph and bullet spacing and the visible summary boundary remain. No extra view, setting, or shortcut was added.

Comment cursor and top-line offset remain independent of panel height, including while results arrive. J/K is needed only when the actual content overflows. The two-thirds limit is an adjustable prototype choice, not a claim about typical user window sizes.

Validation: 62 tests, build, formatting and diff checks passed. Two live model runs fit the complete summary at 120×40, 160×50 and 80×40; the final run left 19, 31 and 14 comment rows respectively. At 80×24 and 40×12, the overflow hint and independent summary scroll remained available. The final PTY run verified comment-anchor preservation during opening and arrival, independent scrolling, paging, source/back, hide/reopen, reply expansion/navigation/collapse (10 → 12 → 10), and clean terminal restoration. The initial runner's reply check used the tiny final viewport and could not observe the count reliably; the completed run restored 120×40 for that check. Captures: `/private/tmp/hn-full-pinned-live/`; runner: `/private/tmp/hn-full-pinned-check.py`. These are real TUI keyboard checks, not native Ghostty mouse/trackpad validation. No commit or push.

### Launching while another session is open

A real launch failed because an existing summary runner already held port 8788. Provider credentials loaded successfully; the old error incorrectly suggested supplying an env file. The launcher now defaults to 127.0.0.1:0, letting the OS allocate a port. The bridge publishes the bound address through a per-launch temporary file (`HN_SUMMARY_READY_FILE`); the launcher health-checks that address and passes it to the TUI. The existing health response cannot convey the address because the launcher needs the address before requesting health. The temporary file is removed on exit. Explicit HN_SUMMARY_ADDR overrides still work. Startup failures now print the bridge log instead of assuming missing credentials.

Premise check: required. The observed failure was a second launch colliding with a live runner. OS listener allocation owns port exclusivity; no retry-port loop, process killing, or reuse of another session's authentication was added.

Validation: shell syntax passed, and the real launcher rebuilt the bridge successfully. With the original runner still on 8788, two live TUI sessions loaded HN feeds on distinct ports. Closing the first left the second able to generate both article and discussion summaries. Both test sessions exited cleanly. Runner: `/private/tmp/hn-launch-check.py`.

### Consistent article shortcut

Feed o now follows Enter into the story workspace and opens or reuses the owned article pane. Reader o retains the same behavior. O opens the regular browser from both views. Help and footer text now distinguish reading the article from explicitly opening the browser. Comments-only c remains unchanged.

Validation: 62 tests, build, formatting and diff checks passed. A live HN PTY run checked feed/reader help, comments-only entry, replies expanding/collapsing 123 → 142 → 123, feed o, reader o, another story via feed o, Enter, owned-pane reuse, and terminal restoration. Browser commands used a local test double so this does not claim native Ghostty rendering or focus validation. A second run exercised the actual macOS external-browser path for feed O, unsupported-terminal o, and reader O. All three Example Domain tabs were confirmed in Chrome and then closed. Evidence: `/private/tmp/hn-feed-open-live/`, `/private/tmp/hn-external-open-live/`; runners: `/private/tmp/hn-feed-open-check.py`, `/private/tmp/hn-external-open-check.py`.

### OpenRouter direction under discussion (not implemented)

The current reviewed proposal is [CLEAN_IMPLEMENTATION_PLAN.md](CLEAN_IMPLEMENTATION_PLAN.md). It supersedes the explicit-S-only trigger below with configurable automatic story-entry generation and adds Fast/Thorough profiles. The notes here describe the earlier proposal.

The current bridge chooses Groq when configured and calls QueryContext with useFasterModel=false. Summarizer's current flexible client maps that to openai/gpt-oss-120b. OpenRouter currently lists that same model and a Groq endpoint; its public endpoint metadata advertises response_format and structured_outputs. Choosing the same model does not establish identical timing or output quality. Pin the provider for a comparison, then decide whether provider preferences are necessary for the user experience.

Recommended first clean path: one Rust CLI with local article extraction, captured HN comments, and direct OpenRouter requests using existing reqwest/serde dependencies. Reader state owns per-story loading/results and citation snapshots. Two concurrent summary tasks return article prose and discussion points independently. An established article-extraction library must replace the private Go dependency; displaying a website in terminal-browser does not provide model input. Keep browser lifetime separate from summarization. Preserve explicit S, full pinned results, and comment position.

A hosted summarizer can later be a second backend returning the same reader-facing article/discussion results. It should be optional, authenticated with its own key, and accessed only through a documented API. No shared private Go package, mandatory daemon, provider registry, database, or job service is needed for the initial OpenRouter path. First validate identical captured sources/prompts against the model, extraction quality on the known difficult pages, and the browser install/close flow on a clean machine.

Sources checked 2026-10-15: https://openrouter.ai/openai/gpt-oss-120b ; https://openrouter.ai/docs/guides/features/structured-outputs ; https://openrouter.ai/docs/guides/routing/provider-selection ; https://openrouter.ai/api/v1/models/openai/gpt-oss-120b/endpoints . No OpenRouter generation request was made.

</details>
