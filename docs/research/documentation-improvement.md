# Research: how should DropTerm's documentation improve, measured against how comparable projects document themselves?

Date: 2026-10-23
Question: *DropTerm ships a 16-page `docs/` tree, a README with a release-install
walk, a cargo-test gate pinning every config key to
[docs/configuration.md](../configuration.md), and a link-checked Jekyll site
that GitHub Pages never serves. What do the closest peers — the pop-os COSMIC
family, community `cosmic-ext-*` apps, the drop-down terminals, and the
gold-standard terminal emulators — actually document, and which of their
practices should DropTerm adopt next?*

## TL;DR

**DropTerm's documentation is already the deepest in its immediate peer group
— richer than every pop-os COSMIC app, every community cosmic-ext README
checked, and the drop-down terminals Guake, Yakuake, and Terra — and its
configuration reference is competitive with the gold-standard terminals; the
five gaps that matter are (1) not a single screenshot or demo anywhere, (2) a
packaging section that documents only the release tarball while its own
packaging research record rules native deb/AUR/nix "in progress", (3) a
polished Jekyll site that cannot be reached because GitHub Pages is disabled
in repo settings, (4) no task-oriented "how-to" layer (Diátaxis: the tree is
reference + explanation, with one how-to hiding in the README), and (5) no
FAQ, changelog, or sample config file — all of which the gold-standard peers
ship and all of which are cheap here because the answers already exist in
DropTerm's own pages.** The pop-os family is the wrong benchmark: cosmic-term,
cosmic-files, cosmic-edit, and cosmic-player all carry minimal
READMEs with no installation instructions at all, so DropTerm's baseline
already exceeds the house convention. The instructive comparisons are kitty,
WezTerm, foot, Ghostty, and Alacritty on structure, and Guake/tdrop on
packaging channels and demos. Eleven ranked recommendations follow; none
conflict with the docs rules in `AGENTS.md` (the config-key gate and
`just docs-check` keep passing; two recommendations explicitly extend them).

The organizing lens is [Diátaxis](https://diataxis.fr/) — used here purely as
a framework (tutorials / how-to guides / reference / explanation as four
user-need-matched forms; "a systematic approach to technical documentation
authoring"), not as an authority about terminals.

## Findings

Each finding cites the exact source fetched. Peer sources were read
2026-10-23; DropTerm paths are from the working tree at the same date.

### 1. The pop-os COSMIC family documents almost nothing — DropTerm's README already exceeds the family convention

- **cosmic-term** ([github.com/pop-os/cosmic-term](https://github.com/pop-os/cosmic-term),
  fetched 2026-10-23): README is two sections — a ~3-paragraph intro ("COSMIC
  terminal emulator, built using alacritty_terminal") and a 2-sentence "Color
  Schemes" section. No screenshot, no installation instructions, no
  keybinding or config docs, no badges, no FAQ, no license section in the
  README body.
- **cosmic-files** ([github.com/pop-os/cosmic-files](https://github.com/pop-os/cosmic-files)):
  one-line pitch ("File manager for the COSMIC desktop environment"),
  build-from-source only (clone / `cargo build --release` / `cargo run
  --release`), no features, no config docs, no screenshots; a "Community and
  Contributing" section and a license section ("This project is licensed
  under GPLv3").
- **cosmic-edit** ([github.com/pop-os/cosmic-edit](https://github.com/pop-os/cosmic-edit)):
  one-line pitch, **an embedded screenshot** (`screenshot-1.png` — the only
  visual asset in the family), pre-alpha disclaimer, build-from-source,
  "Testing" and "Clippy Lints" sections.
- **cosmic-player** ([github.com/pop-os/cosmic-player](https://github.com/pop-os/cosmic-player)):
  placeholder — "WIP COSMIC media player", nothing else.

Versus DropTerm's `README.md`: one-line pitch, layer-shell explanation,
per-arch release-tarball install with checksum verification, automated
dependency gate, shortcut-setup walk, a 16-page Documentation index, feature
list, known risks, non-goals, credits. The only family practices DropTerm
lacks are the cosmic-edit screenshot and the cosmic-files
"Community and Contributing" section.

### 2. Community cosmic-ext apps ship screenshots and a website link; documentation depth is README-only

- **cosmic-ext-tweaks** ([github.com/cosmic-utils/cosmic-ext-tweaks](https://github.com/cosmic-utils/cosmic-ext-tweaks),
  fetched 2026-10-23): tagline ("Beyond the limits of your desktop"), **two
  screenshots** (light/dark, under `res/screenshots/`), dependencies +
  build-from-source (`just build-release` / `sudo just install`), license
  section, a Flathub button, and **a project website link**
  (tweaks.edfloreshz.dev). No feature list, no config docs.
- **cosmic-ext-calculator** ([github.com/cosmic-utils/cosmic-ext-calculator](https://github.com/cosmic-utils/cosmic-ext-calculator)):
  one-line pitch, **light- and dark-mode screenshots**, one feature section
  ("History"), no installation instructions at all.
- **cosmic-ext-quake-terminal** ([github.com/M0Rf30/cosmic-ext-quake-terminal](https://github.com/M0Rf30/cosmic-ext-quake-terminal)):
  the closest sibling project — a Guake/Yakuake-style drop-down for COSMIC
  that wraps an external terminal. README-only docs but task-shaped: "How it
  works", install from source only, a thorough configuration section (config
  path, a six-row table of supported terminals with per-terminal flags,
  extra args), a keyboard-shortcut section with two setup paths and a candid
  known-issue (the same Shift-modified shortcut and GlobalShortcuts-portals
  gaps DropTerm documents), usage/debug logging, license. No screenshot.

Convention takeaway: in the cosmic-ext world, **every polished project ships
at least one screenshot** (cosmic-edit, tweaks, calculator), and the more
mature ones link a website. DropTerm — whose whole value proposition is a
visual gesture (a terminal sliding from the screen edge) — ships none.

### 3. Drop-down terminals: demos and per-distro install pages are the norm; Yakuake and Terra defer to almost nothing

- **Guake** ([github.com/Guake/guake](https://github.com/Guake/guake),
  fetched 2026-10-23): README with pitch, **a screenshot** under "What it
  looks like ?", **three badges** (GitHub Actions CI, ReadTheDocs, Weblate),
  and pointers to a homepage ([guake.github.io](https://guake.github.io),
  with an explicit "Do NOT use the domain guake.org" warning) and a
  **ReadTheDocs site** ([guake.readthedocs.io](https://guake.readthedocs.io)).
  The installation lives on the RTD page
  ([user/installing.html](https://guake.readthedocs.io/en/latest/user/installing.html)),
  organized per channel: Debian/Ubuntu (`apt`, plus a PPA), Fedora (`dnf`),
  Arch and Manjaro (`pacman`), then pipx/`pip install --user` with a warning
  against bare `pip`, then from source with the PBR/git-history caveat. It
  also ships `NEWS.rst` plus a `releasenotes/` directory.
- **Yakuake** ([github.com/KDE/yakuake](https://github.com/KDE/yakuake)):
  README is a pitch ("a drop-down terminal emulator based on KDE Konsole
  technology"), maintainer, links (official site
  [apps.kde.org/yakuake](https://apps.kde.org/yakuake), bug tracker, invent
  source browser), "Basic build and installation instructions" (cmake/make),
  and the license. No screenshots, no keybinding docs in the README — KDE
  defers to the site/handbook.
- **Terra** ([github.com/ozcan/terra-terminal](https://github.com/ozcan/terra-terminal)):
  abandoned ("This repository is outdated please use Sixdsn's fork");
  README documents install channels by distro (Ubuntu PPA, Arch AUR, Gentoo)
  and nothing else. Evidence that per-distro channel lists predate maturity —
  but also that unmaintained channel lists rot.
- **tdrop** ([github.com/noctuid/tdrop](https://github.com/noctuid/tdrop)):
  the best-practice end for a scripted drop-down: **two demo GIFs** under
  "Demos", an "Advantages Over Other Dropdown Terminals" feature section,
  install documented **by channel** ("in the AUR as tdrop-git and is packaged
  for Void Linux", plus `sudo make install`), a basic keybinding example
  (sxhkd), flag documentation deferred to the manpage, dedicated
  "Troubleshooting" and "Why Not Use X?" explanation sections, and Wayland
  limitations stated up front ("I do not recommend using tdrop in Wayland").

### 4. Gold-standard terminals: full doc sites with search, per-option references, FAQs, changelogs

- **kitty** ([sw.kovidgoyal.net/kitty](https://sw.kovidgoyal.net/kitty/),
  fetched 2026-10-23): a Sphinx site whose navigation runs Quickstart →
  Install (binary / build) → Overview (design philosophy) → **Configuring
  kitty** (`kitty.conf` split into per-topic subsections — Fonts,
  Scrollback, Mouse, Tab bar, Keyboard shortcuts — plus a "Sample
  kitty.conf") → "All mappable actions" → ~25 kitten pages → Remote control
  → Sessions → Shell integration → **an ~18-entry FAQ** ("I catted a binary
  file and now kitty is hung?") → a demo video with timestamped chapters →
  a single Changelog page listing every version from 0.4.0 (2017) to 0.49.2
  (2026-10-01) with date anchors.
- **WezTerm** ([wezterm.org](https://wezterm.org/); note
  wezfurlong.org/wezterm now serves only a redirect stub, "Click here to go
  to wezterm.org"): an MkDocs Material site with **search** ("press S or
  click on the magnifying glass icon"), Features sub-pages, per-platform
  Downloads, Configuration sub-pages, a browsable Color Schemes index,
  **Recipes** (task-oriented pages), a "Full Config & Lua Reference", a CLI
  reference, and a "Get Help" group holding Troubleshooting, **F.A.Q.**,
  Getting Help, Contributing, Change Log, Sponsor. One screenshot on the
  homepage.
- **foot** ([codeberg.org/dnkl/foot](https://codeberg.org/dnkl/foot)): the
  README-only counterpoint that still goes deep — Features, Installing
  (deferred to `INSTALL.md`), Configuration (deferred to `foot.ini(5)` man
  page), **Troubleshooting** (deferred to the wiki), **Shortcuts tables**
  (Keyboard — Normal mode / Scrollback search / URL mode — Mouse,
  Touchscreen), Server mode, URLs, Shell integration sub-sections, "No
  AI / LLM policy", Contributing, Code of Conduct, Contact, Sponsoring,
  Credits, License. A **Repology badge** tracks its packaging across distros;
  the repo ships `CHANGELOG.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`.
- **Alacritty** ([github.com/alacritty/alacritty](https://github.com/alacritty/alacritty)):
  pitch, a promo image, features split to `docs/features.md`, install via
  package managers + prebuilt release binaries + `INSTALL.md`, configuration
  via "man 5 alacritty" or the website, a README **FAQ** (performance
  claims; why tabs/splits aren't implemented), license. `CHANGELOG.md` and
  `CONTRIBUTING.md` in-repo.
- **Ghostty** ([ghostty.org/docs](https://ghostty.org/docs) and
  [ghostty.org/docs/config/reference](https://ghostty.org/docs/config/reference)):
  homepage leads with the tagline ("a fast, feature-rich, and cross-platform
  terminal emulator that uses platform-native UI and GPU acceleration") and
  "Featured Documentation" (Keybindings, Color Theme, Configuration,
  Terminal API). The configuration docs open with a **"Zero Configuration
  Philosophy"** ("Zero configuration required to get up and running"),
  document file locations, reload keybinding, offline reference
  (`ghostty +show-config --default --docs`), and link a full **option
  reference** whose entries are per-option headings with prose defaults and
  "Available since: 1.2.0" version annotations.

### 5. Diátaxis reading of DropTerm's tree: reference and explanation are excellent; tutorials and how-tos are nearly absent

Diátaxis ([diataxis.fr](https://diataxis.fr/), fetched 2026-10-23) separates
documentation by user need into four forms: **tutorials** (learning-oriented,
guided first use), **how-to guides** (task-oriented, "how do I...?"),
**reference** (information-oriented, looked up while working), and
**explanation** (understanding-oriented, why it works). Mapping DropTerm:

| Diátaxis form | DropTerm coverage |
|---|---|
| Reference | Excellent: `docs/configuration.md` (all 43 keys: values, defaults, live-vs-next-trigger semantics), `docs/keybindings.md` (action + chords + config id), `docs/cli.md`, `docs/settings.md`, `docs/status-bar.md`, `docs/applet.md` |
| Explanation | Excellent and unusual in kind: 16 research records under `docs/research/` indexed by `docs/research/README.md`, plus `docs/architecture.md` and the "Why some bare keys stay free" section of `docs/keybindings.md` |
| How-to guides | Thin: the README's "Setting up the shortcut" section (`README.md`) and "Running locally without a shortcut" (`docs/development.md`) are the only task-shaped walks; everything else embeds tasks inside reference pages |
| Tutorials | None: `docs/index.md` "Start here" links straight into reference pages (Configuration, Keybindings, CLI) |

kitty (Quickstart), Ghostty ("Get Started", Zero Configuration Philosophy),
and Guake's RTD installing page all provide the first-run path; WezTerm
maintains a dedicated **Recipes** section for the how-to quadrant. DropTerm's
nearest equivalent is the README, which works, but the docs site's "Start
here" mixes forms.

### 6. What DropTerm has today, precisely (grounding)

- `README.md` — pitch, install (v0.2.0 tarball with sha256 verify + ARM
  variant + vendor note + uninstall), Status line, dependency-gated build,
  shortcut setup, **a 16-entry Documentation index**, feature list (four
  thematic bullets), Known risks pointer, Non-goals, Credits. **No image
  anywhere; no badges.** ROADMAP is linked (`README.md` line 33 links
  `ROADMAP.md`; also linked from `docs/configuration.md`'s reserved-keys
  section and `docs/index.md`).
- `docs/` — 16 markdown pages: animation, applet, architecture,
  autosuggestions, bell, cli, configuration, development, index, keybindings,
  known-issues, sessions, settings, status-bar, testing, theming.
- `docs/assets/icon.svg` — the only image asset in the tree; used exactly
  once, as the Jekyll favicon (`docs/_layouts/default.html` line 8). No
  README or docs page embeds it or any other image.
- `docs/configuration.md` — per-key tables with Values / Default / What it
  does columns, live-vs-restart semantics stated per key group, hand-edit
  `echo` examples, profiles RON shape with a per-field "Takes effect" column,
  parked/reserved keys. The strongest single page in the peer set; what it
  lacks versus kitty/Ghostty/WezTerm is a single complete sample config file
  and per-key "since version" annotations.
- `docs/keybindings.md` — action / default chords / action-id table plus the
  layout rationale; stronger than foot's README shortcut tables and matches
  kitty's "All mappable actions" intent.
- `docs/known-issues.md` — 5 upstream-linked rough edges (upstream bug
  tracker items), not user troubleshooting.
- `docs/testing.md` — a 263-line manual checklist; no peer ships anything
  comparable publicly (cosmic family keeps `TESTING.md` unlinked; foot and
  Alacritty keep CONTRIBUTING-level guidance).
- Docs-site mechanics — `docs/_config.yml` (Jekyll for GitHub Pages:
  `url: https://l1br3-io.github.io`, `baseurl: /cosmic-ext-dropterm`, five
  plugins), `docs/_layouts/default.html` (a hand-rolled sidebar layout:
  grouped navigation with active-link highlighting, dark-mode CSS,
  responsive off-canvas nav, an "Improve this page on GitHub" edit link),
  `.github/workflows/pages.yml` (configure-pages → jekyll-build-pages →
  upload → deploy-pages; PRs build without deploying). `docs/_site/` is
  **gitignored** (`.gitignore` line 12; `git ls-files docs/_site` is empty) —
  only the sources are committed. **GitHub Pages is not enabled in the
  repository's settings** (maintainer-known context; a prior `pages.yml` run
  failed at the configure-pages step), so none of this is reachable: the
  published URL appears only inside `docs/_config.yml` itself — the README
  links docs pages as GitHub files, and `docs/index.md` links back to the
  GitHub README/ROADMAP. The layout has no search (static HTML, no JS
  search). Local preview exists: `just docs-serve` (Docker-based Jekyll).
- Link and drift gates — `justfile` `docs-check` (every internal markdown
  link and anchor across `README.md`, `ROADMAP.md`, and `docs/` must
  resolve; external links out of scope), and the cargo test pinning every
  serialized `DropTermConfig` key to `docs/configuration.md`
  (`every_config_key_is_documented`). New root-level or `docs/` markdown
  must satisfy both; a new file outside those paths (e.g. a root
  `CHANGELOG.md`) would not be link-checked unless added to the recipe.
- Release notes — `.github/workflows/release.yml` uses
  `gh release --generate-notes` (auto-generated); there is no `CHANGELOG.md`
  and no in-repo release-notes file. `ROADMAP.md` (checkboxes per version,
  with "V4: improve docs" open) carries the roadmap role.
- Packaging — README documents the tarball and source only;
  `docs/research/packaging-and-release.md` rules "Native deb/AUR/nix
  following the System76 release convention … Flathub for the applet only"
  with status "In progress" in `docs/research/README.md`; the applet Flatpak
  path is documented in `docs/applet.md` (`packaging/flatpak/`,
  `just applet-flatpak-build`).

## Comparison table

Dimensions are the research question's (a)–(l). ✅ present, ⚠️ partial, ❌
absent; DropTerm column cites the repo path.

| Dimension | pop-os COSMIC family | cosmic-ext community (tweaks / calculator / quake-term) | Drop-down terminals (Guake / Yakuake / Terra / tdrop) | Gold standards (kitty / WezTerm / foot / Alacritty / Ghostty) | DropTerm |
|---|---|---|---|---|---|
| (a) README anatomy | ⚠️ pitch only; cosmic-files has license section; no install in 3 of 4 | ✅ pitch; tweaks/calculator have screenshots; quake-term has full task structure; no badges anywhere | ✅ pitch; Guake badges + screenshot; Yakuake/Terra bare | ✅ all; Alacritty promo image + FAQ in README | ✅ full anatomy incl. uninstall + non-goals; no image, no badges (`README.md`) |
| (b) Visual assets | ⚠️ cosmic-edit only (`screenshot-1.png`) | ✅ all three (tweaks 2, calculator 2) | ⚠️ Guake static, tdrop 2 GIFs, Yakuake/Terra none | ⚠️ Alacritty image, WezTerm 1 screenshot, kitty demo video, foot/Ghostty none on README | ❌ zero images; `docs/assets/icon.svg` used only as favicon (`docs/_layouts/default.html:8`) |
| (c) Install & packaging channels | ❌ build-from-source at best (cosmic-files) | ⚠️ source + just; no distro channels | ✅ Guake per-distro page (apt/dnf/pacman/pipx/PPA); tdrop AUR+Void; Terra PPA/AUR/Gentoo | ✅ kitty/WezTerm/Ghostty dedicated install pages; Alacritty INSTALL.md + prebuilts; foot INSTALL.md + Repology badge | ⚠️ tarball + source only (`README.md`); deb/AUR/nix ruled but not shipped (`docs/research/packaging-and-release.md`) |
| (d) Docs architecture (Diátaxis) | ❌ README-only | ❌ README-only | ⚠️ Guake ReadTheDocs site | ✅ full sites; kitty Quickstart, WezTerm Recipes/FAQ/Troubleshooting, Ghostty Get Started | ⚠️ reference+excellent explanation; ~no how-to/tutorial layer (`docs/index.md`, `docs/research/`) |
| (e) Configuration reference | ❌ | ⚠️ quake-term config section only | ❌ (Guake: GUI; config undocumented in README) | ✅ per-option reference, sample configs (kitty `Sample kitty.conf`, foot `foot.ini` template), Ghostty `+show-config --default --docs`, since-version notes | ✅ all 43 keys with defaults/semantics/live-vs-restart, test-gated (`docs/configuration.md`); no sample file, no since-versions |
| (f) Keybindings docs | ❌ | ❌ | ⚠️ Guake GUI; tdrop sxhkd example | ✅ kitty mappable actions; foot README tables; WezTerm key docs | ✅ full table + config ids + rationale (`docs/keybindings.md`) |
| (g) Troubleshooting / FAQ | ❌ | ⚠️ quake-term known-issue section | ⚠️ tdrop Troubleshooting; Guake none in README | ✅ kitty ~18-entry FAQ; WezTerm FAQ+Troubleshooting; Alacritty FAQ | ⚠️ known upstream issues only (`docs/known-issues.md`); no FAQ |
| (h) Changelog / roadmap visibility | ❌ | ❌ | ⚠️ Guake NEWS.rst + releasenotes/ | ✅ kitty/WezTerm changelog pages; foot/Alacritty CHANGELOG.md | ⚠️ ROADMAP.md linked from README; release notes auto-generated only (`.github/workflows/release.yml`) |
| (i) CLI / remote control | ❌ | ❌ | ⚠️ tdrop flags → manpage | ✅ kitty remote control spec; WezTerm CLI reference | ✅ `docs/cli.md` — full verb semantics, exit codes, D-Bus model |
| (j) Docs-site mechanics | ❌ none | ⚠️ tweaks links a website | ✅ Guake ReadTheDocs | ✅ Sphinx/MkDocs sites with search (WezTerm), edit links | ⚠️ Jekyll + workflow + polished layout but **Pages disabled** — site unreachable (`docs/_config.yml`, `.github/workflows/pages.yml`); no search |
| (k) Contributing / dev / testing | ⚠️ TESTING.md unlinked | ❌ | ❌ | ⚠️ foot/Alacritty CONTRIBUTING + CoC | ✅ `docs/development.md` + 263-line `docs/testing.md` + `docs/architecture.md`; no CONTRIBUTING/CoC file |
| (l) Versioning / status signals | ❌ | ❌ | ⚠️ Guake badges | ⚠️ Ghostty "Available since" per option | ⚠️ README Status names v0.2.0; no per-key versions, no badges |

## Recommendations

Ranked by impact vs effort. Each names the peer evidence and the DropTerm
file to change. All are additive except R3's fallback branch; none break the
two gates.

### R1. Ship a screenshot (then a short toggle demo GIF) in the README

**Peer:** cosmic-edit embeds `screenshot-1.png`
(github.com/pop-os/cosmic-edit); cosmic-ext-tweaks and cosmic-ext-calculator
ship light/dark pairs; Guake has "What it looks like ?"; Alacritty a promo
image; tdrop two demo GIFs; kitty a timestamped demo video. Every polished
project in the cosmic-ext cohort shows the product.

**DropTerm:** `README.md` has no image; the only asset is
`docs/assets/icon.svg`, referenced solely as the Jekyll favicon
(`docs/_layouts/default.html:8`). A drop-down terminal's core feature is
visual — the pitch paragraph describes a slide animation in prose
(`docs/animation.md`) that one GIF would replace.

**Effort/impact:** capture needs real COSMIC hardware (per `docs/testing.md`
the runtime only exists there); a static screenshot is low effort, a toggle
GIF medium. Highest first-impression impact in the whole list. Store under
`docs/assets/` beside `icon.svg`; embed in `README.md` near the pitch
(GitHub renders repo-relative images, so no site dependency).

### R2. Decide the GitHub Pages site: enable it, or fall back and stop paying site tax

**Peer:** Guake publishes ReadTheDocs and badges it; kitty/WezTerm/Ghostty
publish searchable doc sites; even cosmic-ext-tweaks links a website. Every
project that invested in a docs tree serves it.

**DropTerm:** the entire pipeline exists and is good —
`docs/_config.yml` (URL, permalinks, plugins), a hand-rolled layout with
sidebar navigation, dark mode, and per-page edit links
(`docs/_layouts/default.html`), the workflow
`.github/workflows/pages.yml`, and a Docker preview (`just docs-serve`) —
but GitHub Pages is not enabled in repo settings, so the site is unreachable
and the URL appears nowhere outside `docs/_config.yml`. Either flip the
settings toggle (then: link the site from `README.md`'s Documentation
section — an external link, so `just docs-check`'s internal-link scope is
unaffected) or retire the workflow and the site chrome. The one thing not to
keep is the current state: a maintained-but-unreachable site whose build
fails in CI.

**Effort/impact:** enabling is a settings toggle plus one README link —
minimal effort, and it unlocks everything `docs/` already contains (the
sidebar nav groups Reference/In depth/Contributing/Decisions are exactly the
right shape). Search would still be absent (static Jekyll, no plugin) —
acceptable at 16 pages; WezTerm-scale search matters at WezTerm scale.

### R3. Add a how-to layer: task pages or a "Common tasks" section (Diátaxis how-to quadrant)

**Peer:** WezTerm maintains a dedicated **Recipes** section (task-oriented
pages); kitty has a Quickstart; Ghostty a "Get Started" +
"Zero Configuration Philosophy" first-run path; Guake's RTD walks the toggle
hotkey setup per desktop. Diátaxis names the gap directly: how-to guides are
task-oriented pages for "how do I...?" needs, distinct from reference.

**DropTerm:** `docs/index.md`'s "Start here" links straight to
Configuration/Keybindings/CLI — reference pages. The real tasks are already
answered but buried: make it drop from the bottom
(`docs/configuration.md` Geometry), set up SSH profiles
(`docs/configuration.md#profiles`), script it from the shell
(`docs/cli.md`), keep it visible while reading (Behavior toggles). A single
new `docs/howto.md` (or three to five short task pages) linking into the
reference pages — not duplicating them — fills the quadrant.

**Effort/impact:** low effort (material exists, needs curation and links);
medium-high impact for new users. Per `AGENTS.md`, any new page must be
added to `README.md`'s Documentation index and passes `just docs-check`.

### R4. Add an FAQ page seeded from questions the docs already answer

**Peer:** kitty ships an ~18-entry FAQ page; Alacritty a README FAQ section;
WezTerm a F.A.Q. under Get Help; foot a wiki Troubleshooting section.

**DropTerm:** `docs/known-issues.md` tracks five *upstream compositor*
issues — real troubleshooting of user-side confusion lives scattered across
pages: "why does my Shift-modified shortcut not fire?" (`docs/known-issues.md`),
"why must the toggle be a COSMIC custom shortcut at all?" (README +
known-issues), "why does the terminal hide when I click away — can I stop
it?" (`hide_on_focus_loss`, `docs/configuration.md`), "why don't
autosuggestions appear over ssh?" (`docs/autosuggestions.md`), "why no
Flatpak?" (README Non-goals + known-issues), "how do I get two independent
instances for development?" (`docs/development.md`). Ten to twelve Q&As
linking into the pages is a page, not a project.

**Effort/impact:** low effort, high search-and-support payoff. Same
`AGENTS.md` rules as R3.

### R5. Document third-party packaging channels when they land; link Repology now

**Peer:** Guake's install page is organized per channel (Debian/Ubuntu apt +
PPA, Fedora dnf, Arch/Manjaro pacman, pipx, source); tdrop documents "the
AUR as tdrop-git … packaged for Void Linux"; Alacritty lists package
managers + prebuilt binaries + INSTALL.md; foot links a **Repology badge**
that tracks its presence across distros automatically.

**DropTerm:** `README.md` documents the tarball and source; its own
`docs/research/packaging-and-release.md` verdict is "Native deb/AUR/nix
following the System76 release convention" with status "In progress" in
`docs/research/README.md`. When those channels land, add them as a short
channel list (exactly Guake's shape). Immediately, a Repology link in the
README costs one line and auto-tracks whatever third parties package —
Terra's rotted PPA/AUR/Gentoo list is the cautionary tale for hand-listing
channels without an aggregator.

**Effort/impact:** trivial now, low when channels land; medium impact
(install friction is the first wall). Immutable-record rule untouched: the
research record stays as-is; only `README.md` gains the channel list.

### R6. Generate and ship a complete sample config, kept honest by the existing test gate

**Peer:** kitty ships a "Sample kitty.conf" and per-tool sample configs;
foot ships `/etc/xdg/foot/foot.ini` as a template the README points to;
Ghostty exposes `ghostty +show-config --default --docs` ("the full default
configuration with docs").

**DropTerm:** `docs/configuration.md` documents all 43 keys, but a user
hand-editing for the first time sees one-file-per-key `echo` examples, not
one complete artifact. Since the `every_config_key_is_documented` test
already walks every serialized `DropTermConfig` key against the page, extend
that same test to emit (or verify) a generated `docs/assets/config-sample/`
snapshot — default value per key in file order — so the sample can never
drift from the code. Alternatively document a `cosmic-ext-dropterm config
dump`-style recipe if a CLI verb is ever added.

**Effort/impact:** medium (touches the test), medium impact; closes the one
real depth gap versus kitty/foot/Ghostty. Flag: the sample must live where
`just docs-check` can see it or be generated, not hand-maintained.

### R7. Add per-key "since" annotations (or version-pinned docs claims) as versions accumulate

**Peer:** Ghostty's option reference annotates availability per option
("Available since: 1.2.0"); kitty's changelog date-anchors every feature so
any option's history is one hop away.

**DropTerm:** `docs/configuration.md` distinguishes parked/reserved keys but
carries no version information; the README's Status section names v0.2.0.
With one released version this is cheap to start (everything is "since
0.1/0.2") and expensive to retrofit later. A `Since` column, or a note
section per release in `ROADMAP.md`, suffices.

**Effort/impact:** low now, annoying later; low-medium impact. No gate
conflict — additive column.

### R8. Add a lightweight changelog or curated release notes

**Peer:** foot and Alacritty ship `CHANGELOG.md`; Guake ships `NEWS.rst` and
a `releasenotes/` directory; kitty and WezTerm maintain changelog pages.
Every gold-standard peer gives release notes a durable, linkable home.

**DropTerm:** `.github/workflows/release.yml` uses `--generate-notes`
(GitHub's auto-list); notes live only on the release page, unedited.
Duplicating kitty's changelog is overkill; a `docs/releases.md` with one
curated section per release (or hand-polished release notes per tag) is
enough, and `ROADMAP.md` (already linked from `README.md` line 33) stays the
forward-looking complement.

**Effort/impact:** low; medium impact for packaged users and for R7. Flag:
if the changelog lives at repo root it is **outside** `just docs-check`'s
file list (`justfile` `docs-check` covers `README.md`, `ROADMAP.md`,
`docs/**`) — put it under `docs/` or extend the recipe, or its links rot
unnoticed.

### R9. Fix the thin/duplicated page: `docs/animation.md`

**DropTerm:** `docs/animation.md` is 5 lines whose lead sentence repeats
verbatim inside the second paragraph ("Showing and hiding slide the whole
content column in from, and back out toward, the anchored screen edge over
about 130 ms" appears twice) — an editing artifact of extracting the lead.
Its content is otherwise complete; it needs one deduplicating pass, not new
material. (Peers have no analogous page to cite — this is internal-quality
housekeeping found by the survey.)

**Effort/impact:** trivial; small. Included because it is the one page that
reads unfinished.

### R10. Add CI and license badges — or consciously skip, citing family convention

**Peer:** Guake carries CI + ReadTheDocs + Weblate badges; foot a Repology
badge. The pop-os COSMIC family and every cosmic-ext app carry none.

**DropTerm:** none (`README.md`). The family convention is no badges, so
this is optional; if CI status is worth signaling (`.github/workflows/ci.yml`
exists), one badge is one line. Lowest rank on purpose — the cohort DropTerm
sits in does not do this.

**Effort/impact:** trivial; low.

### R11. Optional: a CONTRIBUTING pointer

**Peer:** foot ships `CONTRIBUTING.md` + `CODE_OF_CONDUCT.md` and states a
contribution policy ("Use of generative AI/LLMs is strictly forbidden...");
Alacritty ships `CONTRIBUTING.md`; cosmic-files has a "Community and
Contributing" section.

**DropTerm:** the contributor path exists and is good (`docs/development.md`
→ `docs/testing.md` → `docs/architecture.md`, with `AGENTS.md` governing
agents), but there is no one-file pointer a first-time contributor can find.
A short "Contributing" section in `README.md` linking those three pages (the
cosmic-files shape) is enough; a separate CONTRIBUTING.md duplicating them
would violate the fewer-larger-files bias in `AGENTS.md`.

**Effort/impact:** trivial; low-medium.

## Rejected alternatives

- **Adopt the pop-os README shape as a target.** Rejected: the family
  READMEs are minimal because Pop ships the apps in Pop!_OS itself; DropTerm
  is installed by hand and needs the install/shortcut walks it already has.
  The family is the baseline to exceed, not match.
- **Migrate the docs to mdBook/MkDocs with search.** Rejected for now: the
  Jekyll pipeline and layout already exist and are good; at 16 pages search
  is not the bottleneck (WezTerm needs it at hundreds of pages). Revisit if
  the site grows past ~30 pages or R3 adds many task pages.
- **Hand-maintain a per-distro package table today.** Rejected until the
  ruled deb/AUR/nix channels actually exist: a table of channels that are
  not live is worse than none (Terra's rotted PPA/AUR/Gentoo list). Repology
  (R5) covers the interim.
- **Full Diátaxis reorganization — four top-level quadrants.** Rejected as
  ceremony for a 16-page tree; only the missing how-to layer (R3) is
  load-bearing. Renaming existing reference pages into a
  tutorials/how-to/reference/explanation hierarchy would churn every inbound
  link for zero reader gain.
- **Editing existing research records or the decision-log index to reflect
  this survey.** Rejected: records are immutable and status lives in the
  index (`docs/research/README.md`); this record simply joins it, and the
  main session adds the row.

## Sources

- pop-os COSMIC family (read 2026-10-23):
  - cosmic-term — <https://github.com/pop-os/cosmic-term>
  - cosmic-files — <https://github.com/pop-os/cosmic-files>
  - cosmic-edit — <https://github.com/pop-os/cosmic-edit>
  - cosmic-player — <https://github.com/pop-os/cosmic-player>
- Community cosmic-ext (read 2026-10-23):
  - cosmic-ext-tweaks — <https://github.com/cosmic-utils/cosmic-ext-tweaks>
  - cosmic-ext-calculator — <https://github.com/cosmic-utils/cosmic-ext-calculator>
  - cosmic-ext-quake-terminal — <https://github.com/M0Rf30/cosmic-ext-quake-terminal>
  - GitHub search "cosmic-ext" (cosmic-utils org discovery) —
    <https://github.com/search?q=cosmic-ext&type=repositories>
- Drop-down terminals (read 2026-10-23):
  - Guake README — <https://github.com/Guake/guake>; homepage —
    <https://guake.github.io>; installation docs —
    <https://guake.readthedocs.io/en/latest/user/installing.html>
  - Yakuake README — <https://github.com/KDE/yakuake>; official site —
    <https://apps.kde.org/yakuake>
  - Terra — <https://github.com/ozcan/terra-terminal> (marked outdated,
    pointing to Sixdsn's fork)
  - tdrop — <https://github.com/noctuid/tdrop>
- Gold-standard terminals (read 2026-10-23):
  - kitty docs — <https://sw.kovidgoyal.net/kitty/> (navigation, FAQ,
    changelog structure)
  - WezTerm docs — <https://wezterm.org/> (redirect stub at
    wezfurlong.org/wezterm confirmed 2026-10-23)
  - foot README — <https://codeberg.org/dnkl/foot>
  - Alacritty README — <https://github.com/alacritty/alacritty>
  - Ghostty docs — <https://ghostty.org/docs> and
    <https://ghostty.org/docs/config/reference>
- Framework (read 2026-10-23): Diátaxis — <https://diataxis.fr/>
  (cited as the organizing lens only)
- DropTerm working tree (read 2026-10-23): `README.md`, `ROADMAP.md`,
  `AGENTS.md`, `justfile` (`docs-check`, `docs-serve`), `.gitignore`,
  `.github/workflows/pages.yml`, `.github/workflows/release.yml`,
  `packaging/flatpak/` (listing), `docs/_config.yml`,
  `docs/_layouts/default.html`, `docs/assets/icon.svg` (sole image),
  `docs/index.md`, `docs/configuration.md`, `docs/keybindings.md`,
  `docs/cli.md`, `docs/known-issues.md`, `docs/settings.md`,
  `docs/status-bar.md`, `docs/sessions.md`, `docs/animation.md`,
  `docs/theming.md`, `docs/bell.md`, `docs/autosuggestions.md`,
  `docs/applet.md`, `docs/development.md`, `docs/testing.md`,
  `docs/architecture.md`, `docs/research/README.md`,
  `docs/research/packaging-and-release.md`,
  `docs/research/terminal-autocomplete.md` (format precedent).
  Context that GitHub Pages is disabled in repo settings (prior
  `pages.yml` failure at configure-pages) was supplied by the maintainer
  alongside this survey task; the on-disk evidence for the intended site is
  `docs/_config.yml` + `.github/workflows/pages.yml`.
