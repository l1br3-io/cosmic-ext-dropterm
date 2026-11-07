# Autosuggestions

Typing a repeated command finishes itself: as you type, the rest of the most recent matching command from this tab's own history shows as dim ghost text after the cursor — fish's interaction, computed entirely terminal-side.

The rest of the most recent matching command from *this tab's own history* renders as dim ghost text after the cursor as you type. Right accepts all of it, Alt+Right accepts one shell-word, and nothing ever executes without an explicit accept. The rules:

- Right accepts the whole suggestion; Alt+Right accepts one word (leading spaces included); any other key dismisses it. The ghost is inert pixels until an accept key sends it - ignore it and it does nothing.
- History is this tab's own, scraped from the screen as commands finish - and a fresh tab is not starting blind: at spawn it seeds once from the shell's own history file (`~/.bash_history`, `~/.zsh_history`, or `~/.local/share/fish/fish_history` - the default locations, read-only, nothing ever written, nothing leaves the machine). What you run in the tab keeps landing on top as the more recent truth. A tab spawned with autosuggestions off never touches the file, and enabling it later seeds at the next respawn.
- No shell integration, by construction: prompt detection reads the kernel's own foreground truth, so it works in any shell - no scripts shipped, no environment injected, no escape-sequence handling. Over ssh the truth is finer: typing `ssh` hands the foreground to the ssh client, so suggestions stay silent for the whole session; a tab spawned as ssh (a profile command) suggests from its own scraped history - the remote machine's commands, read off the grid.
- Honest silence: no suggestion inside vim or htop (a child holds the foreground), on a `read -p` prompt or a multi-line / right-side prompt (the line no longer matches the prompt anchor), or while scrolled back. Uncertainty always degrades to no suggestion, never a wrong one. Width is the one place where showing less beats showing nothing: a ghost too wide for the columns left on the line clips to what fits with a trailing `…` - fish's own rule - and with fewer than three columns free nothing is drawn while the suggestion stays live for accepting. The clip is display-only: an accept always writes the full text.
- Toggle with `autosuggestions_enabled` (default `true`, Behavior section in the [settings drawer](settings.md)); a flip applies live - a showing suggestion disappears the moment the switch lands, and re-enabling suggests again at the next typed prefix.

## The delay

Every keystroke past the first character flips a guess onto the screen the instant it computes - and while typing fast those guesses are the feature's weakest. `autosuggestions_delay_ms` (default `0`, fish parity - today's behavior exactly) holds a qualifying suggestion back until your typing has been stable for that many milliseconds: no ghost churning while you type, one ghost appearing at the pause. 200-300 ms suits the churn complaint; the settings drawer's Terminal card takes the number and applies it live.

The honesty clause: the delay changes *when* a suggestion is seen, never *which* suggestion wins - the same candidates compute under the same merge order either way, and at `0` the two behaviors are indistinguishable. Any keystroke restarts the wait and dismisses a showing ghost instantly, and Right / Alt+Right are unchanged the moment a ghost shows.

## Paths

Beyond history, a typed path token completes itself from the filesystem: `~/sr` ghosts its completion when one is certain, `./do` ghosts `cs/`, `/usr/bi` ghosts `n/`. The same ghost, the same keys - Right accepts, Alt+Right takes a word, anything else dismisses. History still outranks paths: a command you have run before wins over the directory listing.

The rules specific to paths:

- Any path-shaped token completes: rooted at `~`, `.`, `..`, or `/`, or carrying a `/` anywhere (`sr/x` too). A bare word like `src` never triggers it - that stays history's (and the command catalog's, below) territory.
- A unique match completes fully, directories suffixed `/` and stopping there - one completion, never a menu. An ambiguous field completes only what every match shares: with both `src` and `srv` present, `~/s` ghosts `r` and `~/sr` ghosts nothing - a guess between live alternatives would be wrong half the time.
- Dotfiles (`.`-prefixed names) complete only when your typed component already starts with the dot, matching the shell's own convention.
- Names the shell would mangle are backslash-escaped (`my file.txt` suggests `my\ file.txt`); a printable name that reaches past those sets rides whole inside single quotes instead (`café` suggests `'café'`); and a name neither form can save - a newline, a control character, an embedded quote - suggests nothing at all rather than something wrong.
- Paths are *local-machine* knowledge, so they only run on a tab whose spawned program is a shell (bash, zsh, fish, sh, dash, ksh, csh, tcsh). On a tab spawned as ssh the working directory DropTerm can read is the wrong machine's, so paths stay silent there - history keeps suggesting, being read off the screen; and typing `ssh` inside a tab is already silent end-to-end (above).
- Toggle with `autosuggestions_paths_enabled` (default `true`); a flip applies at the next typed prefix, and `false` restores history-only suggestions exactly.

## Command names

A fresh tab, an empty history: `gi` still finishes itself - to `git`, or whatever the shortest local match is. The command word at the command position completes from the executables on the tab's `$PATH`, snapshotted when the tab spawns and never rescanned. The same ghost, the same keys.

The rules specific to command names:

- Only the command position: the first word of the line, leading spaces aside. Once an argument is open (`git che`), the catalog has nothing to say - that stays history's (and paths') territory.
- Among the names the typed word starts, the **shortest** match wins, ties broken alphabetically: `gi` suggests `git`, not `giftopnm`. The match must extend the word - typing `git` with `git` installed suggests nothing.
- The catalog stays silent below two typed characters - an accuracy ruling, not a toggle: at one character the shortest-match rule over every executable on `$PATH` is a coin flip among hundreds of names, so a fresh tab's `g` ghosts nothing. `gi` → `git` is the floor's own threshold, and history is untouched: on a lived-in tab `g` still completes from the tab's own history.
- History outranks the catalog always: run `git status` once and `gi` ghosts `t status` from then on. The handover is invisible, and the catalog can never replace a suggestion your own past already answered.
- The catalog is *local-machine* knowledge (it lists the spawn-time `$PATH`), so it rides the same shell gate as paths: silent on a tab spawned as ssh or any other non-shell program, while history keeps suggesting. A profile's env-map `PATH` is the PATH it completes from.
- Names the shell would mangle are backslash-escaped (`my cmd` suggests `my\ cmd`); a printable name past those sets rides single-quoted, and a name neither form can save suggests nothing at all.
- Toggle with `autosuggestions_commands_enabled` (default `true`); a flip applies at the next typed prefix, and `false` restores history-only suggestions exactly.

## Subcommands and flags

`git che` ghosts `ckout`. The second word of a command line - the subcommand - and a `--`-prefixed long flag complete from a small curated table: five commands (git, cargo, docker, systemctl, npm), their high-traffic subcommands and long flags, hand-curated and built in. The same ghost, the same keys - and history still outranks the table: a command you have run before wins over any curated entry.

The rules specific to the curated table:

- Subcommands complete only at the first argument seat - the word right after the command. Once a subcommand (or any argument) holds that seat (`git checkout che`), the table has nothing to say; a later word stays history's (and paths') territory.
- Long flags complete from any argument seat: `git --dr` and `git checkout --dr` ghost the same flag, because git genuinely accepts flags before and after subcommands alike. Short flags (`-x`) never complete - the collision space with bundled short flags is guesswork, and a single-choice ghost pays a wrong accept for a wrong guess.
- A bare `--` ghosts nothing - an accuracy ruling, not a toggle: two dashes alone match every long flag in the table equally, so any pick would be a guess with no evidence behind it. One character past the dashes (`--a`) is where a flag prefix starts discriminating, and the curated subcommand seat stays strong even at one character (`git a` ghosts `dd`).
- The match policy is the command catalog's: strict prefix, must extend, shortest match wins, ties broken alphabetically. `git che` suggests `checkout` (the shortest of the table's `che` entries), `docker p` suggests `pull` ahead of `push`, and a fully typed entry suggests nothing.
- Commands outside the five are silent - the provider never guesses. The table is universal knowledge (it describes git, not this machine), but it rides the same shell gate as paths and the catalog: silent on a tab spawned as ssh or any other non-shell program, while history keeps suggesting. `python3 wha` ghosts nothing.
- The table is curated, not derived: nothing runs `--help`, nothing reads man pages, and there is no data file to edit. No toggle yet - the provider is always on wherever its shell gate holds, and its config key is a planned follow-up.

## Aliases

`gc --dr` ghosts git's `--dry-run`: when the command word you type is an alias you have declared, its completions come from the command the alias runs. Declared by you, in the config file - DropTerm never reads your shell's rc files, never runs `alias` in the session, and never guesses an alias it was not told. The map is authoritative for exactly what you declare.

Declare aliases in `autosuggestions_aliases` (default empty - no alias re-targets anything), one expansion per alias, hand-edited in the config file (the map is author-sized, not toggle-sized):

```ron
// ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/autosuggestions_aliases
{
    "gc": "git checkout",
}
```

The rules specific to aliases:

- **Re-target, never rewrite**: the curated-table lookups run against the expansion's command - `gc --dr` ghosts `y-run` and `gc che` ghosts `ckout`, the same ghost, the same keys, the same merge order as typing `git --dr` / `git che`. An accept still writes only the suggestion's suffix; DropTerm never sends the expansion unless you accepted ghost text that spelled it, and your typed `gc` stays `gc` on the line.
- History outranks the map always: after one real `gc checkout main`, `gc che` ghosts `ckout main` from the tab's own truth - the map can never replace a suggestion your own past already answered.
- Chains resolve: an alias may map to another alias (`a = "gc"` follows to git), depth-capped at three expansions and cycle-safe (`a = "b"` with `b = "a"` terminates as unmapped - silence, never a guess).
- The map is *local-machine* truth you declared for your local shell, so it rides the same shell gate as paths, the catalog, and the curated table: silent on a tab spawned as ssh or any other non-shell program, while history keeps suggesting.
- Applies live: a map written, edited, or emptied takes effect at the next typed prefix - a running tab needs no respawn, and a fresh tab picks the map up at spawn.

## Git generators

`git checkout ma` ghosts `in` from the repository's `main`: when you type an argument where a branch, tag, or working-tree change belongs, the generator provider completes it from the repo the tab's shell sits in. `git switch` takes branches, `git checkout` branches and tags, `git add` and `git restore` take your changed files - and the completions menu lists the whole matching set where the ghost settles for one.

The names come from git itself, read under a contract stricter than any peer ships:

- **Fixed, reviewed, read-only commands**: DropTerm runs three exact `git` invocations of its own - local branches, tags, and the short status - the same argv in the source, reviewed and versioned like any code. Nothing you configure - no config key, no data file - can add, alter, or script a generator; there is no knob that could run something on your behalf.
- **Nothing leaves the machine**: the commands read the repo in the tab's own working directory. No network, no telemetry, no external service - a branch name is not data that travels.
- **A scrubbed, reproducible environment**: the `git` binary is resolved once from the tab's spawn-time `$PATH` and run by absolute path in the tab's working directory, with an environment reduced to `HOME` and a pinned `C` locale - DropTerm's own environment never leaks into a generator, and the output stays parseable whatever your desktop locale.
- **Five seconds, then kill, then silence**: a hung generator is killed at the deadline and drops to no candidates. A missing git, a directory that is not a repo, a failing command - every failure is silence, never a dialog, never a wrong completion. VS Code warns; DropTerm stays quiet.
- **Never over ssh**: on a tab spawned as ssh or any non-shell program, the readable working directory is the wrong machine's - generators never run there at all, while scraped history keeps suggesting.
- **Typing never spawns anything**: results cache and serve instantly; the cache refreshes when the completions menu opens and at the tab's spawn. Plain typing - even fast, even long - runs no command.
- A cached name suggests only when no live provider answers: your history always wins, and path and table completions outrank the cache.

Toggle the provider with `autosuggestions_generators_enabled` (default `true`, the Behavior section's Terminal card - "Git completions"). `false` stops every spawn and serves nothing from the cache; a flip applies live, and re-enabling refreshes at the next menu open.

## The menu

`Ctrl+Space` opens a second surface beside the ghost: a candidate list over what you have typed, built from every provider at once - every matching history line, every matching path entry, every matching `$PATH` name, every matching curated subcommand and flag - where the ghost must settle for one choice. The list is scoped to your keyword: `git st` leads with the curated `stash` and `status` and carries the history line after them, and `~/s` over `src` and `srv` leads with both path entries, where the ghost settles for the shared `r`. A bare `git ` - the command typed, the space echoed, nothing after - lists git's whole curated subcommand table above the history lines: the seat rules scope the menu the moment the command word completes, and where the single-choice ghost stays silent for lack of anything to match, the list itself is information. `git checkout ` - a later seat - lists no subcommands; the seat rules below hold at every boundary.

The rules:

- **Open**: `Ctrl+Space` at an eligible prompt - idle, not scrolled back, something typed. At every ineligible state (inside vim or htop, scrolled back, nothing typed) the chord forwards to the shell verbatim: `Ctrl+Space`'s own NUL byte, no menu. The chord is customizable like every keybind (Settings → Keybindings, "Show completions").
- **Filter**: the menu does not need a chord once open - it lists what matches your typed token, re-collected as your line changes.
- **Order - the keyword's scope first**: the list ranks what your typed context already pins down above everything else - the command's own curated subcommands and flags, its cached git names, the typed token's own directory entries - and only then the matching history lines, with command names last at the command seat, where no keyword has settled and this tab's own truth stays the best context. The single-choice ghost keeps the opposite lean - history first - because one silent answer belongs to the tab's own truth, while a list that already carries the seat's scope leads with it.
- **Keys**: Up/Down move the highlight (the list scrolls past eight rows); **Tab and Enter accept the highlighted entry only while the menu shows**; Escape and `Ctrl+Space` close it; any other key closes it first and then reaches the shell normally - the ghost's Right/Alt+Right accepts included. An accept writes the entry's completion at the prompt: typed characters, nothing executes.
- **The bare-Tab promise**: with no menu showing, Tab is the shell's own completion key, byte-for-byte as always - `\x09`, shell tab-completion intact. Tab belongs to the menu only while a menu shows; it is never bound otherwise.
- **Descriptions**: entries render a description beside them when their source carries one (right-aligned, dimmed). No source carries one yet; the curated table grows them as the data-file follow-up lands.
- Toggle the surface with `autosuggestions_menu_enabled` (default `true`, the Behavior section's Terminal card - "Completions menu"); a flip applies live, a showing menu closing the moment the switch lands. The ghost is untouched by either position.
- **Auto-popup, opt-in**: `autosuggestions_menu_auto` (default `false`, the same card - "Open the menu as you type") makes the menu open on its own once your typed input collects two or more candidates, closing again when narrowing leaves one or nothing. No surveyed terminal ships auto-popup on by default - a persistent popup over a grid the terminal does not own buys its keep only for people who ask for it - so `false` keeps `Ctrl+Space` the only opener, exactly today's behavior.
