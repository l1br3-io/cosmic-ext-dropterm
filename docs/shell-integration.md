# Shell integration

The shell tells DropTerm what is happening — and DropTerm listens. With integration, every spawned `bash`, `zsh`, and `fish` reports three facts the terminal otherwise has to guess at: when a prompt is showing (and when a command is running), what a finished command's exit code was, and where the shell's working directory is. The result: the [rich input](rich-input.md) overlays show at every prompt — including the exotic, themed, and multiline ones the grid-read cannot decide — exit codes populate the status band's exit segment on every shell with no configuration, and the path seat follows `cd` through the shell's own report.

Nothing is installed into your rc files. The scripts are DropTerm's own, embedded in the binary and materialized under `~/.cache/dropterm/shell-integration/` (rewritten only when their bytes change); the spawn hands them to the shell through argv and environment alone.

## The wire

The emitters speak two public sequences, the same ones third-party integrations use:

- **OSC 133** — the semantic prompt zones: `A` (the prompt is about to print), `C` (the command is running), `D;<code>` (the command finished, carrying its exit code).
- **OSC 7** — the working directory as a `file://` URL, re-reported at every prompt so `cd` moves the path seat.

On an integrated shell a command's cycle reads: Enter → `C` → the command runs → `D;<code>` → `A` before the next prompt prints. The first prompt of a session reports `A` directly, so the boot prompt is integrated, never guessed at.

## What integrates, and how

The decision runs at spawn and consults the program the PTY actually execs — its basename, never a running-process guess:

| Shell | The mechanism |
|---|---|
| **bash** | Spawned with `--rcfile` pointing at the wrapper; the wrapper sources your `~/.bashrc` whole, then chains the hooks around whatever it set (`PROMPT_COMMAND` runs yours behind the report; `PS0` prints the `C` mark before the command runs). |
| **zsh** | Spawned with `ZDOTDIR` pointing at a bootstrap directory; the bootstrap's `.zshenv`/`.zprofile`/`.zlogin`/`.zshrc` each source your originals (from your real `ZDOTDIR`, defaulting `$HOME`), and `.zshrc` appends the hooks to your own `precmd`/`preexec` arrays. |
| **fish** | Spawned with `-C` sourcing the emitter after fish's own config; the emitter installs fish's own `fish_prompt`/`fish_preexec`/`fish_postexec` event hooks. |

A profile tab (a `ssh web` preset, a `htop` one-shot-then-shell) integrates its **interactive tail only**: the wrap's `exec <shell>` line becomes the integrated spawn, so the preamble command runs bare — no marks, no timing, no verdicts from the preamble phase.

## What stays untouched

- **Other shells and programs** — `sh` (the default fallback), `nu`, `pwsh`, `dash`, any `--cmd` program: spawned byte-identical to today. A `--cmd` line carries its own arguments and never inherits; DropTerm does not decorate your explicit program.
- **Your own file choices** — a configured `args` carrying `--rcfile`, fish's init flag, or a profile env's `ZDOTDIR` suppresses injection for that spawn (one warn in the log): never a fight over whose file the shell reads.
- **The fallback** — no integration means today's behavior exactly: the grid-read decides the prompt (an unsure prompt hides the overlays), exit codes and cwd arrive only if your own shell config emits them. The fallback stands beside integration, never under it.
- **ssh remotes** — the integration is local to the spawned shell; a remote shell over ssh runs whatever the remote has, degrades to the fallback cleanly, and the local prompt re-arms on exit.
- **Foreign integrations** — a shell-integration script you already run (ble.sh, a terminal's own, a hand-rolled one) is consumed as-is: the parsed marks read optional truth, and nothing keys on who sent them.

## The kill switch

`shell_integration` (default `true`, the Terminal page of the [Settings window](settings.md)) skips the whole path when `false`: no scripts materialized, no argv or environment change, byte-identical spawns. The key is read once per spawn — a flip governs the shells spawned after it; a running shell keeps the integration it was spawned with until it exits.

The emitters are written from the public OSC 133/7 specifications; the design survey and the rejected alternative (the 2026-10-02 no-injection ruling this feature reverses) are [in the research log](research/warp-editor-behavior.md).
