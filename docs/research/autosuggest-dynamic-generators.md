# Research: should DropTerm run spawn-based dynamic-value generators for completions, and under what security contract?

Date: 2026-11-02
Question: *[autosuggest-warp-parity](autosuggest-warp-parity.md) (2026-11-02) rejected
spawn-based generators — "a generator is a command DropTerm chose in an
environment it cannot reproduce" — because DropTerm has neither a session
env nor the shell's state, where Warp runs generators through a session it
owns and VS Code through shell-integration-reported env. The owner has
decided to add them anyway: what contract makes DropTerm-side generators
shippable, and which slices of the Fig/Warp generator model must stay
rejected?*

## TL;DR

**DropTerm should supersede the 2026-11-02 rejection and ship spawn-based
generators under a stricter contract than any peer ships: DropTerm-defined
generator programs only (read-only argv constants in DropTerm's own source,
never spec-authorable commands), executed with a scrubbed minimal env in the
tab's live `/proc`-resolved cwd, a 5-second hard timeout with kill, and the
standing shell gate — silent on ssh and non-shell tabs.** The rejection's
core objection — "a command DropTerm chose in an environment it cannot
reproduce" — is answered by making both halves deliberate: the command list
is DropTerm's own reviewed, read-only argv set (the same discipline VS Code
ships for git, pinned below), and the "environment" is reduced to two
reproducible inputs — the spawn-time `$PATH` snapshot the command catalog
already takes, and the tab's cwd the engine already resolves for the path
provider. What stays rejected is the dangerous half of the Fig model: spec
*data* may carry names, flags, and descriptions, but never `script` strings
— withfig specs execute arbitrary author-supplied commands, and that is the
one slice DropTerm does not adopt. First generator set: git only, with
VS Code's exact read-only argv discipline.

## What DropTerm has today

Grounded in the working tree at `fe1cd72`:

- **The engine already resolves everything a generator needs, read-only.**
  `foreground_child_running` (`src/terminal/engine.rs:314-330`) is the
  tcgetpgrp prompt gate; `foreground_process_name`
  (`src/terminal/engine.rs:342-363`) and `foreground_ssh_destination`
  (`src/terminal/engine.rs:388-409`) already walk `/proc/<pid>/comm` and
  `/proc/<pid>/cmdline` (reader at `src/terminal/engine.rs:423-435`) — the
  same machinery that names the foreground process for the status band.
  The path provider's own doc comment names its cwd source as
  `/proc/{shell_pid}/cwd` (`src/terminal/suggest.rs:101-104`), and
  `resolve_token_parent` (`src/terminal/engine.rs:659-703`) already joins
  typed tokens against that cwd — a generator runs in exactly this
  directory, resolved the same way.
- **The provider seam a generator joins.** `step` drives suggestion state
  from an `Observation` (`src/terminal/suggest.rs:294`); four providers
  merge under `source_rank` (`src/terminal/suggest.rs:431`) —
  `history_candidate` (`:445`), `path_candidate` (`:464`),
  `catalog_candidate` (`:528`), `specs_candidate` (`:756`) over the curated
  `SPECS` table (`:575`); `escape_for_shell` (`:804`) is the shared quoting
  tier every suggestion passes. A generator provider slots in as the fifth
  source behind the same shell gate the path/catalog/specs providers
  already ride (the ssh honesty rules in
  [autosuggestions](../autosuggestions.md)).
- **The command catalog's PATH snapshot.** The command-name provider
  completes from "the executables on the tab's `$PATH`, snapshotted when
  the tab spawns and never rescanned"
  ([autosuggestions](../autosuggestions.md)) — the reproducible binary
  source a generator's program resolves against.
- **Config pattern.** Four `autosuggestions_*` keys
  (`src/config.rs:408-451`, defaults at `:505-508`), each serialized,
  documented, and live-flipping.

## Findings

### 1. VS Code's executor: argv spawn, caller-supplied env and cwd, hard timeout — and no allowlist

`extensions/terminal-suggest/src/fig/execute.ts` @ `main` (MIT; read
2026-11-02): generator execution goes through a spawn helper with an
argv-style input — "spawnHelper2(input.command, input.args, …)" — never a
shell string; env and cwd fall back to caller-supplied values
(`env: input.env ?? fallbacks.env`, `cwd: input.cwd ?? fallbacks.cwd`);
the timeout is platform-dependent — "timeout = osIsWindows() ? 20000 :
5000" — wrapped in `withTimeout`, and a timeout rejects into a `catch`
that logs and rethrows; non-zero exit codes are only warned about. The
module contains **no allowlist, no workspace-trust check, no sanitization**
— the security posture is entirely upstream of it (who may author a
generator), which is exactly the seam DropTerm must tighten.

### 2. VS Code's git generators: the read-only argv discipline, pinned line by line

`extensions/terminal-suggest/src/completions/git.ts` @ `main` (MIT; read
2026-11-02): every dynamic value is a fixed argv array, no shell, no env —
branches via `"git", "--no-optional-locks", "for-each-ref",
"--sort=-committerdate"` with a `--format=` template; tags via
`"git", …, "tag", "--list", "--sort=-committerdate"`; commits via
`"git", "--no-optional-locks", "log", "--oneline", "-n", "1000"`; stashes
via `stash list`; changed files via `status --short` and
`diff --name-only`; remotes via `remote -v`; and — notable for the alias
record — **git aliases via `git config --get-regexp "^alias."`**.
Post-processing drops reality noise: lines starting `warning:` or `error:`
are filtered, output starting `fatal:` yields an empty candidate set, and
branch names dedupe through a `seen` Set. This is the exact program set
and flag discipline DropTerm should copy verbatim for its first generator.

### 3. withfig specs: the generator format — and the arbitrary-command slice DropTerm must not adopt

`withfig/autocomplete` `src/git.ts` @ `master` (MIT; read 2026-11-02)
shows the format in full: `script: ["git", "--no-optional-locks",
"branch", "--no-color", "--sort=-committerdate"]` with `postProcess`
factories that "filter fatal errors, dedupe names, and mark the current
branch", `splitOn: "\n"`, async `custom` generators that switch argv by
typed tokens, `insertValue` `{cursor}` placeholders, `fig://icon` URIs —
and a cache field: `cache: { strategy: "stale-while-revalidate" }` on the
git-aliases generator. The format's power and its danger are the same
property: a spec is *code* in data clothing — any author's script string
executes on the user's machine at completion time. VS Code accepts that
by pinning the specs it bundles (the 2026-11-02 record's `cgmanifest.json`
finding); DropTerm's answer is stricter — take the *data* (names, flags,
descriptions), never the `script` fields. Generator programs live in
DropTerm's source, reviewed, versioned, and read-only.

### 4. Warp: generators ride a session DropTerm does not have — but its flag discipline matches VS Code's

Warp's client is open at
[github.com/warpdotdev/warp](https://github.com/warpdotdev/warp) (split
license: AGPL-3.0 for the client, MIT for UI framework crates; client code
unusable in DropTerm, behavior readable). The 2026-11-02 record already
pinned from the client source that Warp executes generators through its
live session machinery with a force-set PATH; Warp's completions docs
(read 2026-11-02) put ~30 commands at Full support and ~420 at Partial
(counted from the page's table) and describe menu matching as "approximate
matches" (fuzzy via approximate string matching) — with **no mention of
AI, models, or cloud anywhere on the completions pages**. Warp's docs also
carry the standing quote that matters for the seam: *"if you have
gc=git checkout in alias, typing gc and hitting Tab should give you the
same completion options as for git checkout"* — alias resolution claims
that live or die with shell-state access (see
[autosuggest-alias-resolution](autosuggest-alias-resolution.md)). No
Warp client source line is cited here for the generator runtime itself;
that finding is carried from the 2026-11-02 record's pinned reading and
marked as such — this record's load-bearing implementation evidence is
VS Code's MIT source.

### 5. fish: completion-time command execution is fifteen years old and entirely unaudited

fish's own completion language has run commands at completion time since
its beginning — `doc_src/cmds/complete.rst` @ `master` (read 2026-11-02):
*"Command substitutions found in ARGUMENTS should return a
newline-separated list of arguments, and each argument may optionally
have a tab character followed by the argument description."* The same
page contains **no security note and no caching guidance** about those
substitutions — the original precedent ships command execution as a
completion primitive with no contract at all. Every later system (Fig,
Warp, VS Code) is a tightening of fish's model; DropTerm's contract below
continues the tightening rather than departing from it.

### 6. The security boundary, stated plainly

The threat model for completion generators has exactly one dangerous
vector: *who chooses the command*. withfig specs let spec authors choose;
fish lets the user's own completion scripts choose (fine — user-authored);
VS Code lets its bundled extension choose (pinned by commit). DropTerm's
contract removes the vector: the generator *set* is compiled into
DropTerm — `git for-each-ref`, `git tag --list`, `git status --short`,
argv constants, nothing else — so no completion data file, spec import, or
remote content can ever introduce a command. What remains is residual
risk common to every implementation including fish's: the helper program
itself (git) reads repository-local configuration when it runs, and runs
as the same user in the tab's own cwd — the identical trust class as the
user typing the same read-only subcommand themselves. `--no-optional-locks`
(the flag VS Code passes on nearly every generator) exists precisely to
keep these subcommands side-effect-light; DropTerm adopts it verbatim.
This is a strictly tighter posture than any surveyed peer: VS Code's
executor has no allowlist, and withfig's format has no boundary at all.

## Recommendation

**Ship generators as a fifth provider behind a DropTerm-defined contract.**
Supersedes the spawn rejection in
[autosuggest-warp-parity](autosuggest-warp-parity.md) (2026-11-02); works
entirely within the standing verdicts — nothing here injects, ships
scripts, or touches OSC.

1. **Programs: DropTerm-owned argv constants, git first.** The first set
   is VS Code's git trio — branches (`for-each-ref` + format template),
   tags (`tag --list --sort=-commercedate`), working-tree files
   (`status --short`) — plus `--no-optional-locks`/`--no-color` on
   everything, `warning:`/`error:` line filtering, `fatal:` → empty, and
   dedupe, exactly as `git.ts` does. docker/systemctl generators are a
   later slice, same contract.
2. **Env: scrubbed to the two reproducible inputs.** Resolve the program
   binary against the tab's spawn-time `$PATH` snapshot; run with a
   minimal env (`HOME`, `LANG`, nothing inherited from DropTerm's own
   process env). This is the objection-answer: the environment is not
   "reproduced" — it is *reduced* until it is reproducible.
3. **cwd: `/proc/{shell_pid}/cwd`, the path provider's own source**
   (`src/terminal/suggest.rs:101-104`). No `cd`, no shell, no expansion —
   `std::process::Command::current_dir` on the resolved path.
4. **Timeout: 5 seconds, kill, silence** — VS Code's non-Windows default;
   on expiry or non-zero exit the candidate set is dropped with no
   user-visible error (the honesty rule: uncertainty degrades to no
   suggestion, never a wrong one — VS Code merely warns; DropTerm stays
   quiet).
5. **Async + stale-while-revalidate.** Generators never block typing:
   results land when they land (withfig's own `cache` strategy field is
   the precedent), keyed by cwd, TTL-bounded, recomputed on menu open.
   A ghost shows a generator result only when it is already cached; the
   Ctrl+Space menu ([autosuggest-warp-parity](autosuggest-warp-parity.md)
   Phase A) is the surface that benefits most, since it opens on demand
   and can wait.
6. **Gate: the standing shell gate.** Generators are local-machine
   knowledge; on a tab spawned as ssh the cwd DropTerm can read is the
   wrong machine's — silent there, exactly as paths/commands/specs are.
   Typing `ssh` inside a tab is already silent end-to-end.
7. **Config: `autosuggestions_generators_enabled` (default `true`)** —
   local-only execution, same class as the path provider's filesystem
   reads, riding the same gate; serialized, documented, live-flipping per
   the existing four keys (`src/config.rs:408-451`).
8. **Spec data stays data.** When the loadable spec file lands
   (warp-parity Phase B), its schema carries names, flags, descriptions —
   and **no `script` field exists to parse**. The `cgmanifest`-style
   discipline (pin the data source by commit) replaces VS Code's
   spec-pinning because the scripts never arrive in the first place.

## Rejected, and why

- **Spec-authorable generator scripts (the Fig format whole).** Any
  format whose data files can carry executable strings reopens the
  arbitrary-code vector the contract exists to close; VS Code manages it
  with commit-pinned manifests, withfig didn't manage it at all. The data
  half imports; the script half never does. Rejected permanently, not
  deferred.
- **Shell-string execution anywhere.** `execute.ts`'s argv discipline is
  the floor: no `sh -c`, no string interpolation into a command line,
  ever — a generator argv is a constant in DropTerm's source.
- **Inheriting DropTerm's process env.** The "environment it cannot
  reproduce" objection reinstated as policy; the scrubbed-env rule makes
  every generator run identical across machines.
- **User-visible errors on generator failure.** A hung or failing
  generator is DropTerm's problem, not the user's prompt's; silence
  preserves the shipped honesty rules verbatim.
- **Generators on non-shell or ssh tabs.** The wrong-machine cwd rule;
  identical to the shipped path/catalog gates, no new carve-out.

Relationship to prior records: **supersedes** the dynamic-values spawn
rejection in [autosuggest-warp-parity](autosuggest-warp-parity.md)
(2026-11-02) — its file-derived allowance stands unchanged; **extends**
[terminal-autocomplete](terminal-autocomplete.md) (2026-10-17) with a
fifth provider; the no-injection verdict of
[terminal-de-integration](terminal-de-integration.md) (2026-10-02) is
untouched — nothing is injected, shipped, or asked of the shell.

## Sources

- VS Code `extensions/terminal-suggest/src/fig/execute.ts` @ `main` (MIT;
  read 2026-11-02) — argv spawn, `env ?? fallbacks`, 5000/20000 ms
  timeout, no allowlist:
  <https://raw.githubusercontent.com/microsoft/vscode/main/extensions/terminal-suggest/src/fig/execute.ts>
- VS Code `extensions/terminal-suggest/src/completions/git.ts` @ `main`
  (MIT; read 2026-11-02) — the generator argv set, `--no-optional-locks`
  discipline, post-filtering:
  <https://raw.githubusercontent.com/microsoft/vscode/main/extensions/terminal-suggest/src/completions/git.ts>
- withfig/autocomplete `src/git.ts` @ `master` (MIT; read 2026-11-02) —
  `script`/`postProcess`/`splitOn`/`cache: stale-while-revalidate`
  format:
  <https://raw.githubusercontent.com/withfig/autocomplete/master/src/git.ts>
- fish `doc_src/cmds/complete.rst` @ `master` (read 2026-11-02) —
  command substitution in `-a` arguments; no security/caching note on the
  page:
  <https://raw.githubusercontent.com/fish-shell/fish-shell/master/doc_src/cmds/complete.rst>
- Warp completions docs (read 2026-11-02) — alias quote, Full/Partial
  table (~30/~420 counted from the page), fuzzy as approximate matching,
  no AI mention:
  <https://docs.warp.dev/terminal/command-completions/completions>
- Warp client repository (split license; AGPL-3.0 client):
  <https://github.com/warpdotdev/warp>; the session/PATH generator-runtime
  finding is carried from [autosuggest-warp-parity](autosuggest-warp-parity.md)
  (2026-11-02), not re-pinned here — marked as such above
- DropTerm local tree @ `fe1cd72` (read 2026-11-02):
  `src/terminal/suggest.rs`, `src/terminal/engine.rs`, `src/config.rs`,
  `docs/autosuggestions.md`; prior records
  [autosuggest-warp-parity](autosuggest-warp-parity.md),
  [terminal-autocomplete](terminal-autocomplete.md),
  [terminal-de-integration](terminal-de-integration.md)
