# The completions seed audit — what may ship from withfig/autocomplete?

Date: 2026-11-03 · Status: the audit gates [`data/completions.toml`](https://github.com/l1br3-io/cosmic-ext-dropterm/blob/main/data/completions.toml)

## TL;DR

The seed ships fourteen commands converted from
**withfig/autocomplete @ `aef52acff84c`** (v2.692.3, 2025-05-05) —
**MIT**, "Copyright (c) 2021 Hercules Labs Inc. (Fig)", verified at
that commit. Every entry is data: a name and, where the source
carries one, a verbatim description. Generators, templates, and
`script` fields never cross the conversion, and the destination
format structurally rejects them (`deny_unknown_fields` — an unknown
key fails the whole file). The MIT notice ships in the seed's header.

## The per-content audit (the terminal-theming precedent)

For every shipped command, the derivation and what crossed:

| Command | Source spec | Shipped | Excluded |
|---|---|---|---|
| apt | `src/apt.ts` | 19 subcommands + 2 flags, all described verbatim | the `packages`/`installedPackages`/`upgradablePackages` generators (two shell-script-bearing); `["autoremove","auto-remove"]` etc. take the primary name |
| cargo | `src/cargo.ts` | the built-in 13 subcommands + 9 flags, described where withfig's top level carries it | `fmt` undescribed (withfig generates it dynamically — nothing static to cite); `--bin`/`--lib`/`--manifest-path`/`--release` undescribed (subcommand-level upstream); the `generateSpec` runtime rebuild; nested subcommands |
| curl | `src/curl.ts` | 29 high-traffic long flags of 225, verbatim | the short-flag pairs of multi-name entries; the `--cert` query generator; 196 further flags (curated out) |
| docker | `src/docker.ts` | the built-in 16 subcommands, described verbatim | withfig's docker spec has **no top-level options** — the built-in flag set ships undescribed; the 42 management nested subcommands; `loadSpec: "docker-compose"` |
| gh | `src/gh.ts` | 19 subcommands + `--help`, verbatim | 26 generator uses, 31 template uses, the `gh alias list` shell command in `parserDirectives`; nested subcommands |
| git | `src/git.ts` | the built-in 24 subcommands, **all** described verbatim, + the built-in 13 flags (`--help`/`--version` described — the rest are subcommand-level upstream) | withfig's 16 further top-level flags (curated out — the five keep their built-in shape); generators; the literal `commit -m 'msg'` pseudo-entry |
| kubectl | `src/kubectl.ts` | 32 of 42 subcommands (the high-traffic set) + `--namespace`, verbatim | `alpha`, `autoscale`, `certificate`, `convert`, `kustomize`, `plugin`, `proxy`, `set`, `taint` (curated out); 81 template uses, 13 generators |
| make | `src/make.ts` | 17 long flags of 23, verbatim | the shell-script-bearing `listTargets` generator; `-j`/`-d` (short-only); `--Next-option` (upstream artifact) |
| npm | `src/npm.ts` | the built-in 8 subcommands, described verbatim | withfig's npm spec has **no top-level options** — the built-in flag set ships undescribed; alias names (`i`, `add`, `tst`, …); nested subcommands |
| pip | `src/pip.ts` | 15 subcommands, described verbatim | upstream ships its top-level options block **commented out** — no flags ship; the 16k-line `packageList` suggestion array; the `pip list` script generator |
| rsync | `src/rsync.ts` | 34 high-traffic long flags of 128, verbatim | the `knownHosts`/`configHosts` shell generators; 94 further flags |
| systemctl | `src/systemctl.ts` | the built-in 7 subcommands + 5 flags, all described verbatim (upstream typos kept — `Supress`) | 61 further subcommands; 43 further flags |
| tar | `src/tar.ts` | 25 long flags, verbatim | withfig models the operation modes as *subcommands*; real tar takes them as long options, so the conversion carries them as **flags** and leaves the subcommand seat empty (completing `tar c` → `reate` would guess at old-style bundling); the `fileOptions`/`dumpOptions` spreads; `-null` (an upstream single-dash typo for `--null`) |
| ~~ffmpeg~~ | `src/ffmpeg.ts` | **not shipped** | withfig's spec is single-dash-shaped (1 long flag in 182 options) — nothing a long-flag table can convert |
| ~~ssh~~ | `src/ssh.ts` | **not shipped** | all 41 top-level options are single-dash; zero convertible content |

## Conversion rules (the regeneration contract)

1. Top-level `subcommands` and `options` arrays only — nested
   subcommands, `args`, generators, templates, and scripts never
   cross.
2. One name per entry: the primary name of a multi-name subcommand
   (`["install","i","add"]` → `install`), the first long form of a
   multi-name flag (`["-v","--verbose"]` → `--verbose`). Short-only
   options drop.
3. Descriptions verbatim, typos included; no description is invented
   — an entry withfig leaves undescribed ships bare.
4. The built-in five re-declare their **complete** built-in entry
   sets (the merge replaces whole; a shrunken seed would shrink every
   table it touches — pinned by
   `the_shipped_seed_parses_and_never_shrinks_the_built_in_five`).

## Regeneration

A one-off, never a build step: fetch the source specs at the pinned
commit (`curl -sfL
https://raw.githubusercontent.com/withfig/autocomplete/aef52acff84c/src/<cli>.ts`)
and re-apply the rules above. The seed's header carries the same note
plus the preserved MIT notice.

## Licensing

withfig/autocomplete is MIT ("Copyright (c) 2021 Hercules Labs Inc.
(Fig)"), verified at the pinned commit `aef52acff84c` on 2026-11-03.
The seed is a substantial derived portion: the notice ships in
[`data/completions.toml`](https://github.com/l1br3-io/cosmic-ext-dropterm/blob/main/data/completions.toml)'s header, and the
derivation of every entry is named above. DropTerm's own code stays
GPL-3.0-only; the data file is the MIT-derived artifact.
