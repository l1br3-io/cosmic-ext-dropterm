# The upstream ask: OSC 133 A/B/C in vte

This carry (`carried/vte`, from `l1br3-io/vte` @ `52dadb7`) exists
because vte 0.15's ansi table parses exactly one FinalTerm semantic
prompt zone marker — `133;D;<code>` (the fork's own one-arm, branch
`dropterm-osc133d`) — and log-drops the other three. This draft is
the ask to carry the remaining arms upstream, so the carry can
retire. **No push has been made** — the ask-first boundary holds;
this file is the draft the carry's header points at.

## The proposal

Parse the three remaining OSC 133 markers beside the existing D arm,
each into a default-no-op `Handler` method — no behavior change for
any embedding that does not override them:

| Bytes | The `Handler` method | Meaning |
|---|---|---|
| `OSC 133 ; A ST` | `semantic_prompt_start()` | The prompt is about to print. |
| `OSC 133 ; B ST` | `semantic_prompt_end()` | The prompt finished (command input begins). |
| `OSC 133 ; C ST` | `semantic_command_start()` | The entered command is running. |

The exact shape of the existing D arm, kept: a bare marker only
(exactly two parameters for A/B/C, three with the numeric exit code
for D); any trailing parameter — contour's `clibel` hints, whatever
an emitter invents — falls through to `unhandled`, and so does a
malformed payload.

## Why upstream should want it

- The D arm set the precedent: the fork's `dropterm-osc133d` branch
  exists because a terminal needed the command-finished report. The
  same consumers need the zone boundaries — prompt truth is the
  shell's word, not a grid inference, and every terminal that
  implements one marker eventually wants the set.
- The default-no-op shape means zero cost for embedders that ignore
  them: the parse table grows three arms, the trait grows three
  methods with empty bodies.
- Contour's specification documents all four markers as one
  protocol; parsing one of four is the anomaly.

## The test story

Five pins in the carry's `src/ansi.rs` (test module) cover the arms
— A (both terminators), C, B's no-op dispatch, and the strict
payload-malformed drop — extending the D arm's own pattern. They
travel with the patch.

## The related asks

- `alacritty/alacritty`: the carried `alacritty_terminal` fork
  implements the D arm on `Term` (emitting `Event::PromptExit`) and
  this module's extension adds `Event::PromptStart` /
  `Event::CommandStart` beside it — the same upstream ask carries
  both events.
- The wide-reflow guards in `carried/alacritty-terminal` (the
  ram-leak record's PR draft) are a separate ask already drafted.

Retirement: the upstream merge, once released — the workspace's
`[patch.crates-io]` re-pins to the registry source and this carry
deletes.
