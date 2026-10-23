# Research: should DropTerm adopt an eager/subtle-style display-mode setting for its autosuggestions, and whose semantics fit best?

Date: 2026-10-24
Question: *The shipped terminal-side autosuggest feature draws fish-style ghost text
immediately whenever the cursor sits at a live prompt and text has been typed, and the
user reports it "feels not accurate." Should DropTerm add a display-mode setting like
Zed's Eager vs Subtle inline-suggestion modes — and if a proven setting exists, which
tool's semantics actually fit a terminal ghost text?*

## TL;DR

**DropTerm should add a display-timing setting, but it should NOT copy Zed's
eager/subtle semantics — verified from Zed's source, "subtle" does not mean "delayed":
it means the inline prediction renders only while the user physically holds a modifier
key (alt by default) and otherwise demotes to a popover/completions-menu surface
DropTerm has no analog for; the vocabulary that maps onto a terminal ghost text is the
idle-delay knob — an "after you stop typing, wait N ms" gate — proven twice at the
exact same layer (single inline suggestion rendered as ghost text): VS Code's
`editor.inlineSuggest.minShowDelay` ("Controls the minimal delay in milliseconds after
which inline suggestions are shown after typing", default 0, range 0–10000, explicit
triggers bypass it) and ble.sh's `complete_auto_delay` (default 1 ms, "The delay to
start auto completion after the user made the last input").** The recommended shape is
one numeric config key — `autosuggestions_delay_ms`, default `0` (exactly today's
behavior, fish parity) — where the ghost is withheld until typing has been idle for N
milliseconds and the accept keys stay live the moment it shows. Zed's own real timing
knob is a different, request-side setting (`edit_predictions.<provider>.prediction_debounce`,
default 75 ms only for Copilot, 0 for Zed's own provider), which corroborates the
delay vocabulary without dragging in Zed's modifier/popover machinery. On accuracy:
no vendor frames a display mode as a fix for wrong suggestions — Zed's subtle mode was
motivated by visual disruption ("so your [buffer] isn't constantly moving as
completions arrive", sic), and JetBrains handles accuracy with filtering ("uses smart
filtering to avoid showing suggestions that tend to be canceled…"); the honest claim
for a delay is that it reduces *exposure* to low-confidence guesses during rapid
typing, which is precisely the regime where DropTerm's shortest-wins catalog/specs
guesses are weakest.

Two corrections to the premise, both load-bearing:

1. **Zed's setting is `edit_predictions.mode`, not `inline_suggestions`.** There is no
   `inline_suggestions` key in Zed's settings today; the eager/subtle vocabulary lives
   under `edit_predictions.mode` (introduced 2025-02, see R1). The names "eager" and
   "subtle" are real; the feature they name is not a timing control.
2. **The accuracy complaint and the display-mode request are related but distinct.**
   Delay settings address when a suggestion is *seen*; they change nothing about which
   suggestion wins. R10 gathers what vendors themselves say about the accuracy angle.

## What DropTerm has today

Grounded in the working tree (read 2026-10-24, HEAD `6b64990`):

- **The ghost shows immediately, on every update, with no delay and no trigger key.**
  `suggest::step` runs inside the engine's grid `update()` on every damage cycle
  (`src/terminal/engine.rs:1683`), and the guard chain (`showing`,
  `src/terminal/suggest.rs:306-331`) returns a `Showing` the moment all guards hold:
  enabled, focused, no foreground child (`tcgetpgrp`), not scrolled back, cursor on
  the bottom row, anchor adopted, a non-empty typed segment, and a candidate from some
  provider. There is no timer, no idle threshold, no minimum prefix beyond
  non-emptiness (`if typed.is_empty() { return None; }`, `src/terminal/suggest.rs:317-319`).
  One keystroke at a prompt ⇒ ghost on the very next wakeup.
- **Four providers, merged first-match-wins in a fixed order**: History (per-tab ring
  seeded from the shell's history file plus live captures) → Path (filesystem listing)
  → Specs (the curated git/cargo/docker/systemctl/npm subcommand+long-flag table) →
  Catalog (spawn-time `$PATH` command names) — `source_rank` and `merged_candidate`
  (`src/terminal/suggest.rs:338-345, 757-772`). Exactly one suggestion or none.
- **Match policy**: strict prefix, must extend the typed text; history takes the most
  recent prefix match; catalog/specs take the shortest match with ties alphabetical
  (`min_by_key(|name| name.len())`, `src/terminal/suggest.rs:448-452, 677-680`).
- **Accept keys are soft**: bare Right writes the whole suffix, Alt+Right one
  fish-shaped word, only while a showing exists (`src/terminal/widget.rs:2359-2365`);
  anything else dismisses. Nothing executes without an accept.
- **Config surface** (`src/config.rs:398-428`): `autosuggestions_enabled` (default
  `true`, the kill switch), `autosuggestions_paths_enabled` (`true`),
  `autosuggestions_commands_enabled` (`true`) — all pinned to `docs/configuration.md`
  by the `every_config_key_is_documented` test — plus the engine-level
  `autosuggestions_specs_enabled` (`src/terminal/engine.rs:128-133`, default `true`,
  still awaiting its config key). There is no display-mode or delay key, and no
  autosuggest keybind (the accept keys are soft, not actions; `src/keybinds.rs` has no
  suggest entry).

So "feels not accurate" lands on a system where every keystroke past the first
character instantly produces exactly one guess, and at 1–3 typed characters the
guesses most likely to be wrong (shortest-catalog-word, any-seat long flags) are the
only ones cheap enough to fire. The question is whether the industry has a proven
setting for that, and what it actually does.

## Findings

### R1. Zed's eager/subtle — verified from source: the modes differ by *suppression surface and a modifier*, not by timing

Primary source: `zed-industries/zed` @ `main` commit `9f305aaf88370e8ca39c702242186e3d8f97ecca`
(2026-10-23). The mode enum, verbatim from
`crates/settings_content/src/language.rs:410-419`:

```rust
#[serde(rename_all = "snake_case")]
pub enum EditPredictionsMode {
    /// If provider supports it, display inline when holding modifier key (e.g., alt).
    /// Otherwise, eager preview is used.
    #[serde(alias = "auto")]
    Subtle,
    /// Display inline when there are no language server completions available.
    #[default]
    #[serde(alias = "eager_preview")]
    Eager,
}
```

The docs say the same thing in user words ([zed.dev/docs/ai/edit-prediction](https://zed.dev/docs/ai/edit-prediction),
read 2026-10-24): *"Edit Prediction has two display modes: eager (default):
predictions are displayed inline as long as they don't conflict with language server
completions; subtle: predictions only appear inline when holding a modifier key (alt
by default)"* — toggleable *"via the mode key"* or *"directly via the UI through the
status bar menu"*, and *"edit prediction modes work with any prediction provider."*

What each mode does in the editor, at the code level:

- **Eager (default)**: the prediction renders inline immediately when it arrives,
  except when the completions machinery is active —
  `crates/editor/src/edit_prediction.rs:836-839`: `let completions_menu_has_precedence
  = !show_in_menu && (self.context_menu.borrow().is_some() || (!
  self.completion_tasks.is_empty() && !self.has_active_edit_prediction()));` — a
  visible completions menu (or completions in flight) suppresses the inline preview.
  That is the whole meaning of "as long as they don't conflict with language server
  completions."
- **Subtle**: `preview_requires_modifier` is set exactly when the mode is Subtle
  (`crates/editor/src/edit_prediction.rs:1592-1594`: `let preview_requires_modifier =
  all_language_settings(file, cx).edit_predictions_mode() == EditPredictionsMode::Subtle;`),
  and the inline preview activates only while the modifier is held —
  `handle_modifiers_changed` in `crates/editor/src/editor.rs:4153-4172` calls
  `update_edit_prediction_preview(&modifiers, …)` which checks
  `edit_prediction_preview_modifiers_held` (`edit_prediction.rs:767-773`). While the
  modifier is *not* held, the prediction is surfaced below the cursor as a popover
  entry instead: `edit_prediction_visible_in_cursor_popover`
  (`edit_prediction.rs:613-629`) returns true when *"we're displaying the edit
  prediction popover below the cursor … like we are not previewing and the LSP
  autocomplete menu is visible or we are in `when_holding_modifier` mode."*

There is **no idle delay and no trigger-key-only mode** in this enum. Neither mode
waits; both display as soon as their condition holds (eager: prediction arrived and no
completion conflict; subtle: alt is held). Timing in Zed lives in a different setting
(R2).

The history pins both the semantics and their stability, all from commit messages and
diffs fetched via the GitHub API (read 2026-10-24):

- Introduced as `inline_completions.inline_preview` with values `auto` /
  `when_holding_modifier` in PR #24316, commit `8ed8b4d2ec` (2025-02-06), whose
  message states the rationale verbatim: *"When set to the latter, instead of showing
  edit prediction previews inline in the buffer, we'll show it in a popover (even when
  there's no LSP completion) so your isn't constantly moving as completions arrive."*
  [sic — "so your isn't"; the referent is the buffer/caret jumping as previews pop in.]
- Renamed to `edit_predictions.mode` (removing the separate
  `editor.show_edit_predictions_in_menu`) in PR #24692, commit `a2592a3a37`
  (2025-02-11).
- Values renamed `auto` → `subtle` and `eager_preview` → `eager` in PR #25657, commit
  `d694458659` (2025-02-26), whose `assets/settings/default.json` diff restates the
  two modes: *"1. Display predictions inline when there are no language server
  completions available. / "mode": "eager" / 2. Display predictions inline only when
  holding a modifier key (alt by default). / "mode": "subtle""*. The default has been
  the eager side the entire time (`#[default]` sits on Eager; before the rename,
  `"mode": "eager_preview"` was the shipped default).
- UI surfaced 2025-03-13 (status-bar menu toggle PR #26680, docs section PR #26683);
  later extended to Supermaven/Copilot completions (PR #35548, 2025-08-07).

### R2. Zed's actual timing knob is a per-provider request debounce — and its defaults are 0

Same commit `9f305aaf`. Typing triggers `refresh_edit_prediction(true /*debounce*/, …,
EditPredictionRequestTrigger::BufferEdit, …)` from the edit path
(`crates/editor/src/editor.rs:5304-5308`), and `refresh_edit_prediction` resolves the
debounce via `debounce_for_delegate`
(`crates/editor/src/edit_prediction.rs:268-281`,
`crates/language/src/language_settings.rs:584-591`). The delay comes from
`prediction_debounce`, defined per provider in
`crates/settings_content/src/language.rs` with doc-comment defaults verbatim: *"The
debounce delay in milliseconds before automatically requesting a prediction after
typing stops. Set to 0 to request predictions immediately."* — Copilot *"Default: 75"*
(:268-269), Codestral *"Default: 150"* (:290-291), and Zed / Mercury / Ollama /
OpenAI-compatible all *"Default: 0"* (:301-302, :312-313, :363-364). The docs page
lists the same table and adds: *"Set prediction_debounce to 0 to disable the
additional delay"* and that the `editor: show edit prediction` command *"bypasses the
debounce entirely."* The setting was made configurable recently — PR #61568, commit
`cb1352a29d` (2026-08-20), "Add configurable inline completion debounce timeout" —
before which the delays were hardcoded per provider.

So even Zed's timing knob is about *when the request fires*, not about gating the
display of an already-computed local suggestion; and Zed's own provider's default is
0 ms. Neither half of Zed's design maps onto DropTerm, whose suggestions are local,
synchronous, and free.

### R3. fish — instant display, boolean-only control, no delay and no prefix threshold

`fish-shell/fish-shell` @ master `9e05df534a6ed4a0bf635aa5eca3c0d5320930da`
(2026-10-16), plus [fishshell.com/docs/current/interactive.html](https://fishshell.com/docs/current/interactive.html)
(read 2026-10-24). fish shows the ghost from the first typed character and offers
users exactly one knob — off:

- `can_autosuggest` (`src/reader/reader.rs:5535-5547`): *"We autosuggest if
  suppress_autosuggestion is not set, if we're not doing a history search, and our
  command line contains a non-whitespace character."* — a non-whitespace gate, not a
  length threshold; there is no suppression at short prefixes anywhere in the file.
- Every text change recomputes asynchronously and immediately: `update_autosuggestion`
  (`src/reader/reader.rs:5616-5625`) — *"Clear the autosuggestion and kick it off in
  the background"* … `self.data.autosuggestion.clear(); …
  self.debouncers.autosuggestions.perform(performer);`. fish's "Debounce"
  (`src/threads/debounce.rs:10-15`) is explicitly *not* a time delay: it *"executes
  one function on a background thread while enqueuing at most one more"*; the
  500 ms constant nearby (`AUTOSUGGEST_TIMEOUT`, `src/reader/iothreads.rs:36-40`) is
  the abandon-a-hung-thread timeout, not a display delay.
- The only user control is the default-on boolean: `fish_autosuggestion_enabled`,
  read at `src/reader/reader.rs:832` (`check_bool_var(…, true)`); the docs' opt-out:
  *"If you don't like autosuggestions, you can disable them by setting
  $fish_autosuggestion_enabled to 0."*

fish is DropTerm's behavioral default (instant, always-on, opt-out) and offers no
display-mode precedent beyond on/off.

### R4. Clink — a surface choice (list vs inline), inline ghost off by default, generation paused while input is pending

`chrisant996/clink` @ master `efc97abb68c4f8fe910ac49261d3db6b98107061` (2026-10-23),
plus [chrisant996.github.io/clink/clink.html](https://chrisant996.github.io/clink/clink.html)
(read 2026-10-24). Clink's answer to "when/how do suggestions appear" is a choice of
*surface*, not timing:

- `autosuggest.enable` (default `true`): *"When this is 'true' then Clink generates
  suggestions automatically based on input. Suggestions may be shown two ways: The F2
  key or the 'suggestionlist.default' setting shows a list of suggestions below the
  input line. … The 'autosuggest.inline' setting shows a suggestion inline after the
  cursor"* (`clink/lib/src/suggestions.cpp:34-53`).
- **`autosuggest.inline` defaults to `false`** — the fish-style muted ghost is opt-in:
  *"The default is 'false'. When both this and 'autosuggest.enable' are 'true' then a
  suggested command may appear in the 'color.suggestion' color after the cursor"*
  (`suggestions.cpp:56-69`); docs add it shows *"when the suggestion list isn't active
  and the cursor is at the end of the input line."*
- `autosuggest.async` (default `true`): *"matches are generated asynchronously for
  suggestions. This helps to keep typing responsive"* (`suggestions.cpp:26-33`).
- The one behavioral throttle is not time: `try_suggest`
  (`clink/lib/src/line_editor_impl.cpp:1724-1731`) bails out under the comment
  *"Don't generate suggestions if Readline is done or there is any pending input"* —
  `if (rl_done || _rl_pushed_input_available() || _rl_input_queued(0)) return;` — so
  suggestion work only starts once the keystroke queue has drained (fast typists see
  suggestions settle after they stop, as a side effect, not a setting).
- No autosuggest delay setting exists; the only ms knob in the area is
  `comment_row.hint_delay` (default 500, max 3000) for the separate Lua input-hints
  feature (docs, read 2026-10-24).

### R5. ble.sh — the shell-side idle-delay precedent for ghost text

`akinomyoga/ble.sh` @ master `d81fd54feb0d996fdff20dca27eaf0201f7015cc` (2026-09-08).
ble.sh's `auto-complete` is fish-style ghost text for bash (accept S-RET; right/C-f/end
at end of line; M-right one word) and it ships a genuine display-delay knob:

- Source defaults (`lib/core-complete-def.sh:52-54`): `bleopt/declare -v
  complete_auto_complete 1` (on for bash 4.0+) and `bleopt/declare -n
  complete_auto_delay 1` — **1 ms**, i.e. effectively immediate but expressible.
- The wiki manual (§7.3.2, [Manual §7 Completion](https://github.com/akinomyoga/ble.sh/wiki/Manual-%C2%A77-Completion),
  read 2026-10-24) documents it: *"The delay to start auto completion after the user
  made the last input. The unit is millisecond."* with default `1`.
- The README's own tuning tip recommends raising it: *"Tip: you may instead specify
  the delay of auto-complete in millisecond"* → `bleopt complete_auto_delay=300`
  (`README.md:569-570`).
- Related display conditions live in `complete_auto_complete_opts` — e.g. the manual's
  *"auto-complete is disabled when the character at the current cursor position, if
  any, is not a newline"* (`suppress-inside-line`) and `suppress-inside-word` — cursor
  position gates, not prefix lengths.

ble.sh is the closest published precedent for "delay N ms after the last input before
the ghost shows" in a *shell* line editor, and it proves the knob coexists with
default-on ghost text.

### R6. VS Code — `editor.inlineSuggest.minShowDelay`: a display-delay knob for inline ghost text, added 2025, default 0

`microsoft/vscode` @ main `02c5a2e2b2e5bb0c1599eea41c153626ca082e14` (2026-10-23).
This is the closest analog to what DropTerm would build, because VS Code inline
completions are the same shape as DropTerm's ghost: a single inline suggestion
rendered as ghost text at the caret, Tab/word-wise accepted.

- The setting, registered in the editor options registry
  (`src/vs/editor/common/config/editorOptions.ts:4654-4660`):
  `'editor.inlineSuggest.minShowDelay': { type: 'number', default: 0, minimum: 0,
  maximum: 10000, description: … "Controls the minimal delay in milliseconds after
  which inline suggestions are shown after typing." }` — alongside
  `editor.inlineSuggest.enabled` (default `true`, *"Enable or disable the rendering of
  automatic inline completions"*, :4503-4507) and `showToolbar`/`suppressSuggestions`.
- Enforcement is a pure display gate, not a request gate:
  `src/vs/editor/contrib/inlineCompletions/browser/model/inlineCompletionsModel.ts:433`
  sets `earliestShownDateTime: requestInfo.startTime + (…triggerKind === Explicit ||
  inAcceptFlow ? 0 : minShowDelay)` — the earliest moment a suggestion may become
  visible; explicitly triggered suggestions (command/menu) bypass the delay entirely.
- History: introduced August 2025 — "Hook up API for `earliestShownDateTime`" (PR
  #261333, commit `fb87057693`, 2025-08-12), then renamed `minimalDelay` →
  `minShowDelay` (PR #261458, commit `47980af437`, 2025-08-13; the diff
  carries the identical description string).
- The companion eagerness vocabulary lives in the Copilot extension:
  `github.copilot.nextEditSuggestions.eagerness` (default `auto`; enum
  auto/low/medium/high) — *"Controls how eagerly next edit suggestions are shown.
  Higher values show more suggestions with less delay."*; `low` = *"Show fewer
  suggestions with longer delays."*, `high` = *"Show more suggestions with minimal
  delay."* (`extensions/copilot/package.json` + `package.nls.json` @ the same commit).

### R7. JetBrains — inline shows as you type with no inline-delay knob; accuracy is a filtering problem; the popup has the delay

JetBrains official help (read 2026-10-24):

- Full Line code completion ([full-line-code-completion.html](https://www.jetbrains.com/help/idea/full-line-code-completion.html)):
  *"Suggestions are displayed in the editor as you type Java, Kotlin, JavaScript/TypeScript,
  or CSS code."* — automatic, no delay or manual-trigger setting; the settings block
  (Settings | Editor | General | Code Completion | Inline) is enable/languages/
  multi-line/synchronize, no timing. And the accuracy stance: *"Full Line completion
  supports auto-import and uses smart filtering to avoid showing suggestions that tend
  to be canceled…"*
- AI Assistant code completion ([ai-assistant/code-completion.html](https://www.jetbrains.com/help/ai-assistant/code-completion.html)):
  *"The suggestions start to appear as you type"*, with a manual fallback ("You can
  also trigger the code completion by pressing Alt+Shift+\"). Its Behavior section
  exposes a **Completion policy** — Creative / Balanced / **Focused** — where
  *"Focused – the default setting. Offers brief, precise suggestions by applying
  strict filters."* An accuracy dial implemented as filtering, not display timing.
- The only delay knob in the completion area is for the *popup*, not inline:
  [auto-completing-code.html](https://www.jetbrains.com/help/idea/auto-completing-code.html)
  — "Show suggestions as you type" (*"Select if you want the suggestion list to be
  invoked automatically"*), with *"In the field to the right, specify the delay (in
  milliseconds), after which the popup should appear."*

### R8. Warp — automatic as you type, toggle only

[docs.warp.dev/terminal/command-completions/autosuggestions](https://docs.warp.dev/terminal/command-completions/autosuggestions)
(read 2026-10-24): *"Warp will automatically suggest commands as you type based on
shell history and possible completions"*; enable/disable *"From the Command Palette,
type in 'Autosuggestions' to toggle"*; the only related settings are the
accept-key remap (*"Tab key behavior"* under Settings > Features > Terminal Input) and
*"Open completions menu as you type."* No delay, no display mode.

### R9. Amazon Q / Fig — dead; nothing left to cite

Re-verified 2026-10-24: `aws/amazon-q-developer-cli` @ main `15cc8f3cd18c4272925ce1c7053268eedff1ea0a`
no longer contains the figterm/fig_desktop autocomplete crates at all (the crate list
is agent/chat clients only); the repository remains "no longer being actively
maintained… Kiro CLI, a closed-source product" per its README, as pinned in
[autosuggest-stage2-crosscheck](autosuggest-stage2-crosscheck.md). There is no
shipping Fig/Q display-mode setting to learn from. Honest gap: any claim about Fig's
inline-suggestion settings would have to come from the archived 2024 client, which is
not a precedent worth adopting.

### R10. The accuracy connection — what vendors themselves say

- **Zed's own rationale for the subtle display is visual disruption, not wrongness**:
  PR #24316 (2025-02-06, quoted in R1): *"…so your isn't constantly moving as
  completions arrive."* [sic]. The mode exists because always-inline previews move
  text under the user; nothing in the PR, the enum docs, or the settings file frames
  it as fixing incorrect suggestions.
- **The "too soon + irrelevant + distracting" framing is on Microsoft's tracker as an
  open feature request**: [microsoft/vscode#319869](https://github.com/microsoft/vscode/issues/319869)
  ("Add a configurable delay for inline suggestions", open, filed 2026-06-04): *"The
  suggestions are normally good if the task is easy - but if you're not sure yet
  about exactly what you want to do, suggestions pop up too soon, and often with
  irrelevant content, which gets very distracting."* — and it asks for exactly the
  idle-delay semantics: *"If I could set, for example, a 2 second delay after I stop
  typing before getting the suggestion, I feel it could fix the problem."* This is
  user wording on the vendor's issue tracker, not a vendor position; the vendor's
  position is the setting description (R6).
- **JetBrains treats noisiness as a filtering problem**: "smart filtering to avoid
  showing suggestions that tend to be canceled" (R7) and the Focused policy's
  "brief, precise suggestions by applying strict filters" — accuracy knobs that never
  touch display timing.
- Net: no vendor claims a display mode makes suggestions more *accurate*. The
  defensible claim — and the one DropTerm's docs should make — is that a delay
  changes *when the single guess is exposed*: during rapid typing the ghost churns
  per keystroke; after a pause the user has declared intent, and one stable guess at
  a longer prefix is both more likely correct (more context typed) and less visually
  noisy. ble.sh's README tip (300 ms) is the shell-side expression of exactly this
  trade.

### R11. The vocabulary survey — what maps onto a terminal ghost text

| Tool | Control | Shape | Maps to DropTerm? |
|---|---|---|---|
| Zed | `edit_predictions.mode` = eager (default) / subtle | suppression surface + hold-alt modifier + popover fallback; needs a completions menu/popover to demote into | **No** — DropTerm has no menu/popover surface, and alt is already the word-accept modifier |
| Zed | `edit_predictions.<provider>.prediction_debounce` | request debounce ms (0/75/150 defaults) | Partially — DropTerm's compute is local and free; the *display* gate is what matters |
| VS Code | `editor.inlineSuggest.minShowDelay` | display-delay ms after typing, default 0, 0–10000, explicit trigger bypasses | **Yes — exact fit** (same single-ghost-text surface) |
| Copilot (VS Code) | `nextEditSuggestions.eagerness` = auto/low/medium/high | coarse spectrum over frequency+delay ("fewer suggestions with longer delays") | Possible naming layer, hides the number |
| ble.sh | `complete_auto_delay` | idle-delay ms before the ghost shows, default 1, README tip 300 | **Yes** — shell-side ghost text, same semantics |
| fish | `fish_autosuggestion_enabled` | boolean only, default on | Already present as `autosuggestions_enabled` |
| Clink | `autosuggest.enable` + `autosuggest.inline` (false) + F2 list + `autosuggest.async` | surface choice; ghost is opt-in; pending-input gate | Partially — DropTerm has no second surface; the pending-input gate is a free implementer detail |
| JetBrains | popup autopopup delay; inline: none (filtering policies instead) | ms delay for the popup only | No — different surface |
| Warp | palette toggle only | boolean | Already present |
| min-prefix-chars thresholds | **nobody** ships one | — | Honest gap: no precedent found in any source above; fish explicitly suggests from the first non-whitespace character (R3), ble.sh gates on cursor position not length (R5) |

The two vocabularies with real precedent at DropTerm's layer are the **idle-delay
ms** (VS Code, ble.sh) and the **eagerness spectrum** (Copilot NES, which is itself a
packaging of delay+frequency). "Eager/subtle" as a *word pair* is Zed's, but Zed's
*semantics* are the one shape that does not transfer.

## Recommendation

**Adopt VS Code's `minShowDelay` semantics, spelled as one numeric DropTerm config key
— `autosuggestions_delay_ms`, default `0` — not Zed's mode enum.** Graded by
precedent: the behavior (ghost text withheld until the input has been idle for N ms;
0 preserves fish parity exactly; accept keys unaffected; any keystroke resets the
timer) is proven twice at the ghost-text layer — VS Code's `minShowDelay`
(description, default, range, and explicit-trigger bypass all cited in R6) and
ble.sh's `complete_auto_delay` (default 1 ms, README tip 300 ms, R5). Concretely:

1. **Semantics** (all local, no timers beyond one deadline): while a suggestion
   qualifies under the existing guard chain, the `Showing` is withheld (not computed
   differently — the model is pure and free) until the typed segment has been stable
   for `autosuggestions_delay_ms`; any PTY damage that changes `before_cursor`, or a
   foreground/scroll/focus transition, restarts the clock. Right/Alt+Right behave
   exactly as today the moment the ghost is visible; a keystroke that would dismiss
   also cancels the pending show. Default `0` = today's behavior bit-for-bit, so the
   feature ships as an opt-in remedy, the same default posture as VS Code (`default:
   0`) and ble.sh (`1`, effectively immediate).
2. **Key and docs**: one serialized key `autosuggestions_delay_ms: u32` in
   `DropTermConfig` beside the other autosuggest toggles (`src/config.rs:398-428`),
   following the numeric-knob precedent of `command_finish_notification_after_secs`
   (`src/config.rs:457`); documented in `docs/configuration.md`
   (`every_config_key_is_documented` gate) and `docs/autosuggestions.md` with the
   honest wording: this changes *when* the suggestion appears, not *which* suggestion
   wins — the accuracy levers are the per-provider toggles and the match policy.
   A sensible suggested value for "feels noisy" reports is 200–300 ms (ble.sh's own
   documented tip is 300).
3. **What the delay buys, honestly**: the 1–3-character regime — where the catalog's
   shortest-word rule and the specs table's any-seat long flags produce their
   weakest guesses — collapses into "ghost appears only once you pause," which is
   both the fish/ble.sh interaction rhythm and the moment a single-choice guess is
   most defensible. It does not make any individual suggestion more correct (R10);
   if the accuracy complaint persists at pause, the next lever is provider-side
   (e.g. a catalog/specs minimum word length or fish-style validation), which no
   peer currently ships as a user setting (R11's honest gap).
4. **Implementation shape** (small, pure-model): add `delay_ms` to the observation or
   the engine's suggest state as an earliest-show deadline set on each typed-segment
   change; `showing()` gains one guard (deadline reached). No provider, merge, or
   accept-path change; the existing per-update `step()` call
   (`src/terminal/engine.rs:1683`) already provides the clock ticks (wakeup-driven
   updates), so a deadline compare needs no new timer thread.

If the maintainer prefers a *named* mode over a raw number, the defensible packaging
is Copilot's eagerness spectrum (R6) — `autosuggestions_display: "eager" |
"balanced" | "relaxed"` mapping to 0/300/800 ms — because that vocabulary is
published as a delay packaging, unlike Zed's. The raw ms key is recommended first:
it is the smaller surface, it matches both VS Code and ble.sh exactly, and mode
names that promise more than "a delay" would mislead.

## Rejected, and why

- **Zed's exact two-mode semantics (`edit_predictions.mode` eager/subtle).** Verified
  from source (R1): subtle = inline only while holding alt, with a popover/menu
  fallback. DropTerm has no completions menu or popover to demote the suggestion
  into, its ghost is already computed locally with zero arrival latency (nothing
  "lands late" to hold back), and Alt is load-bearing as the word-accept modifier —
  alt-hold-to-preview would collide with Alt+Right. Copying the names while changing
  the semantics (subtle := delayed) would borrow Zed's vocabulary for someone else's
  meaning. Rejected as specified; the honest import is Zed's *timing* setting (R2),
  which is a debounce with default 0 — i.e. Zed itself, on its own provider, ships
  the eager default this record keeps.
- **A min-prefix-chars threshold (show only after N characters).** No precedent found
  in any primary source surveyed (R11): fish suggests from the first non-whitespace
  character, ble.sh gates on cursor position, VS Code/Zed/Clink/JetBrains gate on
  delay, surfaces, or filters — none on typed length. It would also kill the
  catalog's one undeniably correct case (`gi` → `git` on a fresh tab, the exact
  scenario `autosuggestions_commands_enabled` documents in
  `docs/configuration.md:179`). Rejected for lack of precedent and clear regression.
- **Clink's surface split (F2 list as the default, inline ghost opt-in).** Proven at
  scale in Clink, but it is a different product shape: Clink already owns a
  completion UI above the line; DropTerm's verdict from
  [terminal-autocomplete](terminal-autocomplete.md) is inline-first with a popup as a
  later, separate decision. Making the ghost opt-in would flip DropTerm's
  default-on fish posture for no accuracy gain. Rejected as a display-mode answer;
  Clink's *pending-input* gate (R4) is worth stealing as an implementation detail if
  the delay key ever proves insufficient.
- **An eagerness enum as the primary setting.** Defensible (Copilot NES publishes
  exactly this, R6) but strictly a packaging of the delay number; it hides the
  ms from users who know what they want and adds a vocabulary to document. Keep as
  the fallback if raw-ms tests poorly with users; not the first ship.
- **Doing nothing (the accuracy complaint is a provider problem, not a display
  problem).** Partially true — R10 shows no vendor claims display modes fix
  correctness — but the user-requested remedy exists, is cheap, is twice-precedented
  at this exact layer, and defaults to off. Refusing to ship the knob while the
  complaint stands would be principled about accuracy and unhelpful about the actual
  ask.

## Sources

All fetched 2026-10-24 unless noted.

- Zed: `zed-industries/zed` @ `main` commit `9f305aaf88370e8ca39c702242186e3d8f97ecca`
  (pinned via API 2026-10-24; commit dated 2026-10-23) —
  `crates/settings_content/src/language.rs` (`EditPredictionsMode` enum :410-419;
  `EditPredictionSettingsContent.mode` :158-162; per-provider `prediction_debounce`
  doc-defaults :206-207/:268-269/:290-291/:301-302/:312-313/:363-364),
  `crates/editor/src/edit_prediction.rs` (mode→`preview_requires_modifier`
  :1592-1594; completions-menu precedence :836-839; popover visibility :613-629;
  debounce resolution :268-281; preview-on-modifier :767-773),
  `crates/editor/src/editor.rs` (typing→debounced refresh :5304-5308;
  `handle_modifiers_changed` :4153-4172), `crates/language/src/language_settings.rs`
  (`debounce_for` :560-577, `debounce_for_delegate` :584-591) —
  <https://github.com/zed-industries/zed>
- Zed history commits (messages + diffs via GitHub API, 2026-10-24): #24316
  `8ed8b4d2ec` (2025-02-06, "Preview while holding modifier mode" + rationale),
  #24692 `a2592a3a37` (2025-02-11, rename to `edit_predictions.mode`), #25657
  `d694458659` (2025-02-26, auto→subtle / eager_preview→eager), #26680 `d4eab557b2`
  + #26683 `1034d1a6b5` (2025-03-13, menu + docs), #61568 `cb1352a29d` (2026-08-20,
  configurable debounce)
- Zed docs, "Switching Modes" + debounce table —
  <https://zed.dev/docs/ai/edit-prediction> (read 2026-10-24)
- fish: `fish-shell/fish-shell` @ master `9e05df534a6ed4a0bf635aa5eca3c0d5320930da`
  (2026-10-16) — `src/reader/reader.rs` (:832 default-on; :5535-5547 `can_autosuggest`;
  :5616-5625 clear-and-recompute), `src/threads/debounce.rs` (:10-15 coalescing, not
  timing), `src/reader/iothreads.rs` (:36-40 abandonment timeout) —
  <https://github.com/fish-shell/fish-shell>; docs —
  <https://fishshell.com/docs/current/interactive.html>
- Clink: `chrisant996/clink` @ master `efc97abb68c4f8fe910ac49261d3db6b98107061`
  (2026-10-23) — `clink/lib/src/suggestions.cpp` (:26-33 async; :34-53 enable/two
  surfaces; :56-69 inline default false; :89-101 strategy),
  `clink/lib/src/line_editor_impl.cpp` (:1724-1731 pending-input gate) —
  <https://github.com/chrisant996/clink>; docs —
  <https://chrisant996.github.io/clink/clink.html>
- ble.sh: `akinomyoga/ble.sh` @ master `d81fd54feb0d996fdff20dca27eaf0201f7015cc`
  (2026-09-08) — `lib/core-complete-def.sh:52-54`, `README.md:567-570` —
  <https://github.com/akinomyoga/ble.sh>; wiki Manual §7 Completion (auto-complete,
  complete_auto_delay default 1) —
  <https://github.com/akinomyoga/ble.sh/wiki/Manual-%C2%A77-Completion>
- VS Code: `microsoft/vscode` @ main `02c5a2e2b2e5bb0c1599eea41c153626ca082e14`
  (2026-10-23) —
  `src/vs/editor/common/config/editorOptions.ts` (`IInlineSuggestOptions` :4503-4534;
  defaults :4594-4612; `minShowDelay` registration :4654-4660),
  `src/vs/editor/contrib/inlineCompletions/browser/model/inlineCompletionsModel.ts:433`
  (`earliestShownDateTime` gate), `extensions/copilot/package.json` +
  `package.nls.json` (`nextEditSuggestions.eagerness` strings) —
  <https://github.com/microsoft/vscode>; history commits #261333 `fb87057693`
  (2025-08-12), #261458 `47980af437` (2025-08-13); open feature request
  <https://github.com/microsoft/vscode/issues/319869> (filed 2026-06-04)
- JetBrains help (read 2026-10-24): Full Line code completion —
  <https://www.jetbrains.com/help/idea/full-line-code-completion.html>; AI Assistant
  code completion —
  <https://www.jetbrains.com/help/ai-assistant/code-completion.html>; Code completion
  (autopopup delay) —
  <https://www.jetbrains.com/help/idea/auto-completing-code.html>
- Warp docs, Autosuggestions —
  <https://docs.warp.dev/terminal/command-completions/autosuggestions> (read
  2026-10-24)
- Amazon Q / Fig: `aws/amazon-q-developer-cli` @ main `15cc8f3cd18c4272925ce1c7053268eedff1ea0a`
  (crate inventory checked 2026-10-24 — autocomplete crates removed); status per
  [autosuggest-stage2-crosscheck](autosuggest-stage2-crosscheck.md) (2026-10-21)
- DropTerm local tree (read 2026-10-24, HEAD `6b64990`): `src/terminal/suggest.rs`
  (guards :306-331; merge :338-345, 757-772; catalog/specs match :448-452, 677-680),
  `src/terminal/engine.rs` (step call :1683; specs option :128-133),
  `src/terminal/widget.rs` (:2359-2365 accept keys), `src/config.rs` (:398-428, :457,
  :481-483), `docs/configuration.md` (:63-65, :177-179);
  prior records [terminal-autocomplete](terminal-autocomplete.md) (2026-10-17) and
  [autosuggest-stage2-crosscheck](autosuggest-stage2-crosscheck.md) (2026-10-21)
