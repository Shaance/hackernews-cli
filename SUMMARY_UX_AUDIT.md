# Summary workflow UX audit

2026-10-15. Scope: the clean implementation proposal and an interactive concept. The running Rust prototype is unchanged. This is a heuristic audit, not a claim of validated model quality or a finished product.

## Outcome and evidence

An HN reader opens a story to understand its argument, compare the discussion, inspect a cited comment, and continue reading without managing generation or losing position.

- Observed: Chris repeatedly rejected extra expansion, unclear readiness, reset scroll position, and scattered entry points. He chose a full summary pinned above comments, a browser companion pane, automatic generation, and two model preferences. The current plan and prototype code provide evidence about request ownership, citation selection, and retry behavior.
- Inferred: ordinary navigation should retain useful submitted work; selecting Thorough likely expresses an immediate desire to improve the open summary.
- Assumed: Fast is the initial default; profile changes apply to the current story and future summaries; two independent section completions are sufficient without token streaming. These are editable product choices.
- Unknown: actual OpenRouter latency, price and quality differences; the best extractor; fresh-machine browser installation; native terminal accessibility. Exact model recipes remain open. Native Ghostty validation is unavailable in this environment.

## Material findings and incorporated corrections

### 1. P2: navigation discards useful work

Moment: A → B → A while generation is unfinished. Observation: the original plan canceled unfinished local work on departure, even though the provider might keep processing it. Consequence: repeated waits and possibly duplicate requests merely from browsing. Evidence: observed plan rule; user impact inferred.

Correction incorporated: submitted requests finish into their owning story for this session. Reuse them on return. Stop obsolete preparation before inference and bound local concurrency; only the latest active story waits for an available slot. No persistent queue or feed prefetch.

Verification: with article A ready and discussion A pending, leave and return. The article remains, and the original discussion request continues or has completed. Navigation adds no duplicate requests. The mock demonstrates retention; real request-count verification belongs to implementation.

### 2. P2: setup hint lacks a complete recovery path

Moment: first story without credentials or with a rejected key. Observation: the plan specified a hint, but no complete action/save/return behavior. Consequence: setup becomes another interruption rather than leading directly to first value. Evidence: observed omission; impact inferred.

Correction incorporated: S Set up summaries opens one settings surface with Fast and Auto on selected. Connect returns to the same story and generates when Auto is on. Cancel preserves position. Authentication failures offer Edit key; temporary failures offer Retry. Existing configured keys are reused. No extra confirmation.

Verification: scroll comments, open settings, cancel, reopen, connect, and recover from a rejected key. The mock uses a read-only sample credential and simulates success. It neither accepts nor stores a real API key.

### 3. P2: selected profile and displayed result can disagree

Moment: upgrade Fast → Thorough, partial replacement, failure, and return. Observation: the original plan left revisit behavior tentative and did not fully specify Auto changes or failed replacement. Consequence: users cannot tell which summary they are reading or whether the choice took effect. Evidence: observed ambiguity; impact inferred.

Correction incorporated: show each section's actual profile, a pending target, and an explicit failure retaining the previous result. The picker says For this story and future summaries. Revisit deterministically reuses matching work, starts missing or mismatched work with Auto on, and never retries a prior failure automatically. Auto on starts missing current work; Auto off affects future starts. Choosing a retained profile supersedes an unnecessary replacement.

Verification: Fast ready → Thorough → article succeeds/discussion fails → leave/return → retry → switch back. Profile labels, pending state, visibility, and comment anchor must remain coherent. No quality or timing claim is established by the simulated mock.

### 4. P2: successful but old summaries cannot refresh

Moment: a story first summarized before comments arrive. Observation: completed work is reused, while retry only repairs failures. Consequence: users can be stuck with an empty or outdated discussion unless they misuse profile switching. Evidence: observed plan and prototype empty-discussion outcome; impact inferred.

Correction incorporated: one Refresh summary action within existing settings. Recapture sources with the selected profile and retain old results until replacements succeed. Capture time and coverage appear next to the section. No automatic refresh timer or freshness settings.

Verification: start with a successful empty discussion, refresh after comments exist, then verify same-profile replacement without position loss. A failed refresh retains the old content and offers retry.

### 5. P2: source/back can return to a stale citation selection

Moment: a summary replacement finishes while a cited source is open. Observation: the original plan protected source text but left the parent summary and selection unspecified. The prototype selects citations by list position. Consequence: returning can make Enter open an unrelated source. Evidence: observed state representation; consequence inferred.

Correction incorporated: keep the captured source and its parent stable while open. Stage a discussion replacement. Publish on return, clear the obsolete citation selection, and say Summary updated. Preserve the separate live-comment anchor.

Verification: open a Fast source, finish a Thorough replacement with changed citation order/count, return, then press Enter. No unrelated source may open from the stale selection. The concept demonstrates a stable source and explicit update on return; native keyboard selection remains an implementation acceptance check.

All five findings were incorporated in CLEAN_IMPLEMENTATION_PLAN.md. No additional material findings were withheld. No P0 or P1 was established from the available evidence.

## Friction and scope

| Moment | Demand | Class | Treatment |
|---|---|---|---|
| First use | Supply a provider credential | Necessary, once | One settings surface; reuse configuration; resume reading |
| Every story | Request, wait, reopen summary | Accidental | Auto on; full pinned result appears as each section completes |
| Navigation | Repeat unfinished work | Accidental | Retain submitted requests within the session |
| Upgrade | Understand what changed | Necessary, occasional | Explicit scope and per-section result labels |
| Later discussion | Get newer sources | Necessary, occasional | Contextual refresh; previous result remains readable |
| Reading | Manage summary height | Accidental if forced | Content-sized panel; independent overflow controls only when needed |

Keep one reader, one summary settings surface, two model preferences, and two independently completing sections. Defer a provider registry, background job service, model catalog UI, result history, token streaming, automatic refresh, and browser crawler. A private summarizer backend remains a later API adapter rather than a dependency of the public CLI.

## Rubric before and after correction

Scores describe the documented design, not product performance. The fresh reviewer supplied the corrected-plan column. They are not averaged into an approval score.

| Dimension | Initial / 3 | Corrected plan / 3 |
|---|---:|---:|
| User and outcome clarity | 2 | 3 |
| Value and product coherence | 3 | 3 |
| Context reuse and question economy | 3 | 3 |
| Whole-journey coherence | 1 | 3 |
| Time to value | 2 | 3 |
| Effort and decision load | 2 | 3 |
| Defaults and progressive disclosure | 2 | 3 |
| Comprehension and feedback | 1 | 3 |
| Control and recovery | 2 | 3 |
| AI autonomy and trust | 2 | 2 |
| Accessibility and inclusion | 2 | 2 |
| Privacy and protective friction | 2 | 2 |
| Reliability and operational fit | 2 | 2 |
| Measurement and learning | 1 | 3 |

The remaining 2s reflect unverified model fidelity/cost, credential storage, native accessibility, extraction, and deployment behavior. A browser concept cannot settle them.

## Outcome checks

| Goal | Signal and measure | Decision rule |
|---|---|---|
| Get useful orientation with little effort | Open a configured story; count actions before first useful section | No extra S or expansion. If Fast repeatedly lacks useful context, revisit its recipe/default |
| Preserve reading | Track first visible comment through arrival, hide, upgrade, source/back, refresh, and revisit | Any reset fails acceptance |
| Make the model choice legible | Ask which profile produced each visible section during partial replacement | Confusion means change status/labels before adding choices |
| Avoid repeat spend and waiting | Count provider submissions during A → B → A and feed movement | No duplicate because of return, no feed prefetch, bounded concurrency |
| Improve summary quality | Compare Fast and Thorough on the same captured inputs for fidelity, useful nuance, wait, and cost | Keep two profiles only if a meaningful tradeoff exists; names alone are not evidence |

No analytics infrastructure is proposed. Start with direct observation and request logs in the next prototype run.

## Review and validation record

Codex was already authorized for this goal. This turn used two independent Codex subagent runs: initial deep UX audit and one fresh re-review after corrections. Root synthesized the plan and built the mock. No further review loop was started.

The fresh reviewer found no P0/P1 and no new plan-level P2. It found three P2 discrepancies in the mock, all corrected and confirmed through the browser:

- Credential repair cleared an authentication error without starting work. Repair now resumes missing sections with Auto on; verified the rejected-key scenario progresses directly to generation while preserving comment 2.
- Refresh ignored unapplied profile selection. It now applies the visible settings and starts one refresh per section; verified Fast → select Thorough → Refresh yields Thorough results and newer discussion while preserving comment 2.
- Feed bypassed the staged citation-result handoff. Any departure from the source view now publishes its completed replacement; verified Fast source → Thorough completes → Feed → reopen displays ready Thorough content immediately.

Browser inspection also verified first-use cancel/connect, manual mode and S, full-summary hide/show, j navigation, upgrade partial failure and targeted retry, article failure and retry, source/back with an update notice, A → B → A, and independent article-pane close/reopen. The active comment index remained unchanged across the exercised summary operations. Keyboard Escape from settings and focus after a disappearing retry control were corrected during inspection. Own widget-state echoes no longer reset local interactions.

Rendered checks covered 1024- and 736-pixel browser widths and a 320-pixel content width, plus light and dark palettes. DOM geometry showed no horizontal overflow at the checked medium and narrow sizes. The narrow preview stacks the article below the reader to remain usable in chat; it is not a proposal for a new native terminal layout. Simulated comment navigation advances the visible comment sample; it does not validate real terminal wheel scrolling or constrained-height overflow.

The three fresh-review findings were confirmed fixed by the root's bounded browser confirmation. No unresolved core-journey P2 remains in this heuristic audit. No real OpenRouter requests, real credentials, model comparisons, native screen-reader tasks, or Ghostty operations were performed. The public CLI implementation and production acceptance gates remain ahead.
