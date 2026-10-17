# Autosuggestions

Typing a repeated command finishes itself: as you type, the rest of the most recent matching command from this tab's own history shows as dim ghost text after the cursor — fish's interaction, computed entirely terminal-side.

The rest of the most recent matching command from *this tab's own history* renders as dim ghost text after the cursor as you type. Right accepts all of it, Alt+Right accepts one shell-word, and nothing ever executes without an explicit accept. The rules:

- Right accepts the whole suggestion; Alt+Right accepts one word (leading spaces included); any other key dismisses it. The ghost is inert pixels until an accept key sends it - ignore it and it does nothing.
- History is this tab's own, scraped from the screen as commands finish: nothing is read from disk, nothing leaves the machine, and a respawned tab starts empty.
- No shell integration, by construction: prompt detection reads the kernel's own foreground truth, so it works in any shell - no scripts shipped, no environment injected, no escape-sequence handling. Over ssh the truth is finer: typing `ssh` hands the foreground to the ssh client, so suggestions stay silent for the whole session; a tab spawned as ssh (a profile command) suggests from its own scraped history - the remote machine's commands, read off the grid.
- Honest silence: no suggestion inside vim or htop (a child holds the foreground), on a `read -p` prompt or a multi-line / right-side prompt (the line no longer matches the prompt anchor), or while scrolled back. Uncertainty always degrades to no suggestion, never a wrong one.
- Toggle with `autosuggestions_enabled` (default `true`, Behavior section in the [settings drawer](settings.md)); a flip applies live - a showing suggestion disappears the moment the switch lands, and re-enabling suggests again at the next typed prefix.

## Paths

Beyond history, a typed path token completes itself from the filesystem: `~/sr` ghosts the first sorted match in your home directory, `./do` ghosts `cs/`, `/usr/bi` ghosts `n/`. The same ghost, the same keys - Right accepts, Alt+Right takes a word, anything else dismisses. History still outranks paths: a command you have run before wins over the directory listing.

The rules specific to paths:

- Any path-shaped token completes: rooted at `~`, `.`, `..`, or `/`, or carrying a `/` anywhere (`sr/x` too). A bare word like `src` never triggers it - that stays history's (and the command catalog's, below) territory.
- The first match in sorted order wins, the matched entry minus what you typed, directories suffixed `/` and stopping there - one completion, never a menu.
- Dotfiles (`.`-prefixed names) complete only when your typed component already starts with the dot, matching the shell's own convention.
- Names the shell would mangle are backslash-escaped (`my file.txt` suggests `my\ file.txt`); a name containing anything outside the conservative safe sets - a newline, a control character, unusual bytes - suggests nothing at all rather than something wrong.
- Paths are *local-machine* knowledge, so they only run on a tab whose spawned program is a shell (bash, zsh, fish, sh, dash, ksh, csh, tcsh). On a tab spawned as ssh the working directory DropTerm can read is the wrong machine's, so paths stay silent there - history keeps suggesting, being read off the screen; and typing `ssh` inside a tab is already silent end-to-end (above).
- Toggle with `autosuggestions_paths_enabled` (default `true`); a flip applies at the next typed prefix, and `false` restores history-only suggestions exactly.

## Command names

A fresh tab, an empty history: `gi` still finishes itself - to `git`, or whatever the shortest local match is. The command word at the command position completes from the executables on the tab's `$PATH`, snapshotted when the tab spawns and never rescanned. The same ghost, the same keys.

The rules specific to command names:

- Only the command position: the first word of the line, leading spaces aside. Once an argument is open (`git che`), the catalog has nothing to say - that stays history's (and paths') territory.
- Among the names the typed word starts, the **shortest** match wins, ties broken alphabetically: `gi` suggests `git`, not `giftopnm`. The match must extend the word - typing `git` with `git` installed suggests nothing.
- History outranks the catalog always: run `git status` once and `gi` ghosts `t status` from then on. The handover is invisible, and the catalog can never replace a suggestion your own past already answered.
- The catalog is *local-machine* knowledge (it lists the spawn-time `$PATH`), so it rides the same shell gate as paths: silent on a tab spawned as ssh or any other non-shell program, while history keeps suggesting. A profile's env-map `PATH` is the PATH it completes from.
- Names the shell would mangle are backslash-escaped (`my cmd` suggests `my\ cmd`); a name outside the conservative safe sets suggests nothing at all.
- Toggle with `autosuggestions_commands_enabled` (default `true`); a flip applies at the next typed prefix, and `false` restores history-only suggestions exactly.
