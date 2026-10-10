# Research: how should DropTerm package, distribute, and release for COSMIC in late 2026?

Date: 2026-10-10
Question: *How do users actually install third-party COSMIC apps, what does each
distribution channel (Flathub / the COSMIC Flatpak repo / deb / AUR / nix) demand from
a Rust workspace with a git-pinned libcosmic, can the out-of-process panel applet ship
in a Flatpak, and what does a healthy v1.0 release process look like for a single
maintainer?*

## TL;DR

**The COSMIC "app market" is Flathub plus the deb world, with one big carve-out that
decides everything: apps belong on Flathub, panel applets belong in System76's
`pop-os/cosmic-flatpak` repository, and DropTerm's own drop-down surface makes
Flathub impossible for the main binary anyway.** The official routing is written in
libcosmic's own README — *"Applications may be submitted to Flathub. Desktop-specific
components—such as COSMIC applets—are not permitted on Flathub so make sure to submit
them to the COSMIC Flatpak repository instead"*
([libcosmic README](https://github.com/pop-os/libcosmic/blob/master/README.md)) — and
that repository
([pop-os/cosmic-flatpak](https://github.com/pop-os/cosmic-flatpak), 56 applets as of
2026-10-10) is offered inside the COSMIC Store as the recommended "COSMIC Flatpak"
source. Out-of-process applets (like DropTerm's `cosmic-ext-dropterm-applet`) work as
Flatpaks by design: cosmic-panel discovers applet desktop files through XDG data dirs,
detects that a desktop file came from a Flatpak export, and spawns it via Flatpak with
the panel's Wayland socket inherited. But DropTerm's **terminal window is a
`zwlr_layer_shell_v1` surface, and cosmic-comp binds layer-shell only for clients
without a Wayland security context** — so a sandboxed main binary gets no layer shell
and cannot show the drop-down at all. That is not speculation; it is exactly why the
`cosmic-ext-applet-app-switcher` applet was measured, found broken, and removed from
the store
([cosmic-flatpak #259](https://github.com/pop-os/cosmic-flatpak/issues/259)). The
recommended path for v1.0 is therefore: **native-first** — GitHub tag + release
tarball (the System76 `just vendor` → `vendor.tar` convention), a `cargo-deb` deb for
Pop!_OS users, an AUR package (which already has a direct precedent in
`cosmic-ext-quake-terminal`), and **the applet submitted to pop-os/cosmic-flatpak** so
store-first users can install it; Flathub for the main app only if a non-layer-shell
fallback mode ever exists, and nixpkgs as a cheap follow-on (it already packages 11+
`cosmic-ext-*` apps via a documented hook).

## Findings

### 1. The COSMIC "app market": cosmic-store browses Flathub AND installs debs

The store on Pop!_OS 24.04 is
[pop-os/cosmic-store](https://github.com/pop-os/cosmic-store) (cloned 2026-10-10,
`README.md`: "WIP COSMIC Store"). Its backend layer is explicitly multi-kind —
`src/backend/mod.rs` defines `BackendName::{FlatpakUser, FlatpakSystem, Packagekit,
Pkgar, RpmOstree}` — so the same UI installs Flatpaks and distro packages
(PackageKit covers apt/deb; pkgar and rpm-ostree cover System76's immutable plans).
Two details verified in source:

- **Flathub is pushed as the default Flatpak source.** `src/main.rs:1914` embeds
  `res/flathub.flatpakrepo` as a `SourceKind::Recommended` entry, and
  `src/priority.rs:25-26` special-cases it: "Among flatpak-user sources, the flathub
  source is preferred". The UI carries an `enable-flathub-cosmic` localization string
  (`src/view.rs:1219`) — the store prompts to enable Flathub.
- **The COSMIC Flatpak repo is a first-class store source.** Immediately after the
  Flathub entry, `main.rs` adds a second recommended source named "COSMIC Flatpak"
  with `res/cosmic.flatpakrepo` (URL `https://apt.pop-os.org/cosmic/`, GPG key
  "Pop OS (ISO Signing Key)"), with `requires: vec!["flathub"]` because its
  `com.system76.Cosmic.BaseApp` base depends on Flathub-hosted runtimes.

System76's support docs confirm the user-facing behavior: on current Pop!_OS,
"Flathub plus the Ubuntu repositories are the default software sources listed in the
COSMIC Store", both Flatpak and .deb versions of an app can be offered, and "apps not
listed can be installed by downloading a 64-bit .deb file and double-clicking it,
which opens the Store with an install option"; the Repoman tool manages additional
Flatpak repos
([system76.com/support/managing-apps](https://system76.com/support/managing-apps)).
(UNVERIFIED: whether Pop!_OS 24.04 COSMIC is branded alpha/beta/stable at this exact
date — the release-notes page moved and returned 404; nothing in this research
depends on it.)

So "the app market" for DropTerm is: **Flathub (apps) + COSMIC Flatpak repo
(applets) + apt/deb + whatever the user adds by hand** — and cosmic-store handles all
of the first three natively.

### 2. The Flathub route: process, manifests, and the applet question

#### 2.1 Submission process (active model, verified October 2026)

The documented, live process for **new** apps is still the GitHub model: follow the
requirements, build locally with org.flatpak.Builder, lint with `flatpak-builder-lint`,
then open a PR titled "Add <appid>" **against the `new-pr` branch of
`flathub/flathub`**; after review a new per-app repo is created under the flathub org
and updates from then on are plain commits to it that trigger server-side builds
([docs.flathub.org — submission](https://docs.flathub.org/docs/for-app-authors/submission)).
Direct evidence the model is alive today: new per-app repos were still being created
in the flathub org on 2026-10-07/08 (e.g. `io.github.word_sys.word-sys-pdf-editor`,
`net.blip.Blip`), and their histories show bot PRs merging
(`gh api orgs/flathub/repos?sort=created`, checked 2026-10-10). A **direct-upload**
model also exists (upload prebuilt repos via the Flathub web portal); it is not in
the docs' submission flow, but the MetaInfo guidelines acknowledge it: "Direct
uploads (no Flathub GitHub repo) must add `flathub::manifest` location in a
`<custom>` tag"
([metainfo-guidelines](https://docs.flathub.org/docs/for-app-authors/metainfo-guidelines)).
For DropTerm the GitHub model is the right default: it keeps the manifest in a
reviewable repo and matches every COSMIC app already there.

Requirements that bite DropTerm specifically
([docs.flathub.org — requirements](https://docs.flathub.org/docs/for-app-authors/requirements)):

- **App ID**: `io.github.l1br3_io.CosmicExtDropterm` is valid — reverse-DNS, ≥4
  components for `io.github.`, dashes converted to underscores in the domain
  portion (`l1br3-io` → `l1br3_io`), must exactly match the MetaInfo `<id>`. The
  protected-prefix table explicitly reserves `com.system76.` for the pop-os org
  (DropTerm correctly avoids it).
- **Builds are offline**: "No network during build; all deps as sources … dependency
  manifests required (npm, cargo, pip)". This is why cargo-sources.json exists (2.3).
- **Inclusion policy**: no tray-only apps, no shell extensions, no
  "environment-locked" apps, stable releases only, meaningful development history,
  and — new since 2025 — an AI policy: manifests must contain no AI-generated
  content and "AI agents must not open PRs".
- **Licensing**: GPL-3.0-only is fine ("Content must be legally redistributable");
  license files must be installed to `$FLATPAK_DEST/share/licenses/$FLATPAK_ID`.

#### 2.2 A real COSMIC app manifest on Flathub

com.system76.CosmicTerm does **not** exist on Flathub (404 at
<https://flathub.org/apps/com.system76.CosmicTerm>; System76 ships COSMIC apps as
Pop debs only — finding 3.2). The COSMIC apps that ARE on Flathub (from Flathub's own
AppStream index, `dl.flathub.org/repo/appstream/x86_64/appstream.xml.gz`, fetched
2026-10-10) include `dev.edfloreshz.CosmicTweaks`, `dev.edfloreshz.Tasks`,
`io.github.cosmic_utils.Examine`, `com.vkhitrin.cosmicding`,
`io.github.cosmic_utils.camera`, `io.github.pixeldoted.cosmic-ext-color-picker`.
The best-studied example, `com.vkhitrin.cosmicding` (Rust + libcosmic with git
deps), manifest at
[flathub/com.vkhitrin.cosmicding](https://github.com/flathub/com.vkhitrin.cosmicding/blob/master/com.vkhitrin.cosmicding.yaml):

```yaml
runtime: org.freedesktop.Platform
runtime-version: "24.08"
base: com.system76.Cosmic.BaseApp        # System76's shared COSMIC base (branch: stable)
base-version: stable
sdk: org.freedesktop.Sdk
sdk-extensions: [org.freedesktop.Sdk.Extension.rust-stable]
finish-args:
  - --share=ipc
  - --share=network
  - --socket=fallback-x11
  - --socket=wayland
  - --device=dri
  - --talk-name=com.system76.CosmicSettingsDaemon   # COSMIC theming daemon
  - --filesystem=xdg-config/cosmic:rw               # libcosmic config (watch_config)
sources:
  - cargo-sources.json                  # flatpak-cargo output: every crate + git dep
  - type: git {url: https://github.com/vkhitrin/cosmicding.git, tag: v2025.11.1}
```

The finish-args pattern is stable across COSMIC Flatpaks — `Examine`
([flathub/io.github.cosmic_utils.Examine](https://github.com/flathub/io.github.cosmic_utils.Examine/blob/master/io.github.cosmic_utils.Examine.json),
runtime 25.08) uses `--socket=wayland --socket=fallback-x11 --device=all
--talk-name=com.system76.CosmicSettingsDaemon --filesystem=xdg-config/cosmic:ro`.
So a Wayland COSMIC app needs: wayland socket (+fallback-x11 for X11 users), dri
device (GPU rendering; `--device=all` is the blunter variant), settings-daemon
talk-name (theme hot-reload), and `xdg-config/cosmic` access. A terminal additionally
needs the host-shell story from §8.1 and — if SSH from inside the sandbox should
work — `--socket=ssh-agent` (a standard Flatpak socket;
[flatpak sandbox permissions](https://docs.flatpak.org/en/latest/sandbox-permissions.html)
documents the socket set). Desktop file, icon, and metainfo are installed to
`/app/share/{applications,icons/hicolor,metainfo}/` — visible to the host because
Flatpak exports them (`$FLATPAK_ID.desktop` etc., rewritten Exec per
[flatpak conventions](https://docs.flatpak.org/en/latest/conventions.html)).

#### 2.3 Building a git-pinned libcosmic workspace with flatpak-builder

There is no network in the Flatpak build sandbox, so `Cargo.lock`'s crates.io AND
git dependencies must be pre-materialized as manifest sources. The standard tool is
`flatpak-builder-tools/cargo` (flatpak-cargo-generator):
`python3 flatpak-cargo-generator.py Cargo.lock -o cargo-sources.json` produces a
sources file that vendoring builds consume via
`CARGO_HOME=/run/build/<module>/cargo` + `cargo --offline build`
([flatpak-builder-tools/cargo README](https://github.com/flatpak/flatpak-builder-tools/tree/master/cargo)).
Git deps come out as pinned `{"type":"git", "url": …, "commit": <rev>, "dest":
"flatpak-cargo/git/<name>-<shortsha>"}` entries — verified in the wild in
`dev.cappsy.CosmicExtAppletDict`'s cargo-sources.json, which materializes
`pop-os/libcosmic`, `pop-os/cosmic-protocols`, `pop-os/cosmic-panel-config`, …
exactly the way DropTerm's pinned `08a09e3` rev would
([cosmic-flatpak/app/dev.cappsy.CosmicExtAppletDict](https://github.com/pop-os/cosmic-flatpak/tree/master/app/dev.cappsy.CosmicExtAppletDict)).
DropTerm's two-crate workspace needs both crates' graphs in one cargo-sources.json
(the generator walks the whole lockfile, so one file covers the applet member too).

#### 2.4 CRITICAL: can an out-of-process COSMIC panel applet ship in a Flatpak? YES — the platform supports it explicitly; cosmic-comp is the part that doesn't

The applet is an ordinary executable discovered by desktop-file stem and spawned by
the panel as a separate process (established in
[docs/research/v3-panel-applet.md](./v3-panel-applet.md), findings 1-2). Every step
of that works for Flatpaks:

- **Discovery**: the panel iterates
  `freedesktop_desktop_entry::default_paths()` matching `file_stem() == name`
  (`cosmic-panel-bin/src/space/wrapper_space.rs:465-475` @ `3ce6ce1`, 2026-10-06).
  `default_paths()` is XDG_DATA_HOME + XDG_DATA_DIRS + `applications`
  ([pop-os/freedesktop-desktop-entry `src/lib.rs:678-685`](https://github.com/pop-os/freedesktop-desktop-entry/blob/main/src/lib.rs)),
  and Flatpak's own session setup prepends
  `~/.local/share/flatpak/exports/share` and `/var/lib/flatpak/exports/share` to
  XDG_DATA_DIRS (`profile/flatpak.sh` and `env.d/flatpak.env.in` in
  [flatpak/flatpak](https://github.com/flatpak/flatpak)). Exported desktop files
  land exactly there
  ([flatpak conventions](https://docs.flatpak.org/en/latest/conventions.html)),
  and their `Exec` is rewritten at install to
  `/usr/bin/flatpak run --branch=stable --arch=x86_64 --command=<orig> <appid>`.
- **Detection + spawn**: the panel computes `PanelClient::is_flatpak()` via
  `PathSource::guess_from(path)` matching `SystemFlatpak | LocalFlatpak`
  (`cosmic-panel-bin/src/space/panel_space.rs:203-209`), and when true injects
  `--die-with-parent`, `--socket=inherit-wayland-socket`, and
  `--env=<k>=<v>` for every panel env var into the (already `flatpak run`-shaped)
  Exec (`wrapper_space.rs:620-633`). `--socket=inherit-wayland-socket` is the
  documented, deliberately powerful option that passes the inherited
  `$WAYLAND_SOCKET` fd into the sandbox — "a very sensitive permission as it
  prevents Wayland client state from being sandboxed"
  ([flatpak sandbox permissions](https://docs.flatpak.org/en/latest/sandbox-permissions.html)).
  On respawn the socket is refreshed through `--env=WAYLAND_SOCKET=…`
  (`wrapper_space.rs:781-788`). The applet therefore connects to the **panel's**
  nested Wayland display, never to cosmic-comp directly — which is why it needs no
  cosmic-comp protocols and is sandbox-safe.
- **Picker**: cosmic-settings lists installable applets from the same
  `default_paths()` enumeration filtered on `X-CosmicApplet`
  (`cosmic-settings/src/pages/desktop/panel/applets_inner.rs:71,571`), so a
  Flatpaked applet shows up in the panel editor like a native one.
- **Precedent**: 56 third-party COSMIC applets currently ship exactly this way in
  [pop-os/cosmic-flatpak](https://github.com/pop-os/cosmic-flatpak) — e.g.
  clipboard-manager (236★ upstream), minimon, status-hub, the cappsy applets.

**The counter-case is cosmic-comp, and it hits the MAIN app, not the applet.**
cosmic-comp binds sensitive protocols only for non-sandboxed clients:
`WlrLayerShellState::new_with_filter::<State, _>(dh, client_not_sandboxed)` at
`cosmic-comp/src/state.rs:738` (master `4d14528`, 2026-10-08), where
`not_sandboxed()` treats any client carrying a Wayland security context as
sandboxed, with one carve-out — engine `com.system76.CosmicPanel` is trusted
(`state.rs:165-174`), which is precisely how panel-attached Flatpak applets stay
first-class. A Flatpak's own security context does not get that treatment. DropTerm's
drop-down is built on libcosmic's layer-surface API
(`src/surface.rs:11-16` imports `layer_surface::{Anchor, KeyboardInteractivity,
Layer as WlrLayer}`; `wlr_layer()` maps Overlay/Top) — so a Flatpaked main binary
would simply never receive `zwlr_layer_shell_v1` in its `wl_registry` and could not
create the drop-down. This is the exact, measured failure documented when the
app-switcher applet was withdrawn: flatpak ≥1.15 "always attaches" a security
context, `zcosmic_toplevel_info_v1`/`zwlr_layer_shell_v1` disappear from the
sandbox's registry (58 → 36 globals on cosmic-comp `7ee78d9`, Flatpak 1.16.6), and
`--filesystem=/usr/share/applications:ro`-style entries are silently ignored
("`/usr` is reserved by Flatpak")
([cosmic-flatpak #259](https://github.com/pop-os/cosmic-flatpak/issues/259)). The
author's only workaround was opting out of the Wayland sandbox entirely
(`--nosocket=wayland` + host socket), "which defeats the point of shipping it as a
Flatpak".

Conclusion: **applet → Flatpak OK (via cosmic-flatpak repo); main terminal → Flatpak
NOT viable on COSMIC today.** (UNVERIFIED: whether cosmic-comp will grow an
intended path for sandboxed layer-shell clients — the #259 author reports asking;
nothing has landed in master as of `4d14528`.)

#### 2.5 The cosmic-flatpak submission flow (the applet's home)

[pop-os/cosmic-flatpak](https://github.com/pop-os/cosmic-flatpak) README: "This
repository hosts applets and other flatpaks for COSMIC that are not suitable for
upload to Flathub. For COSMIC apps, please first try to submit to Flathub." Users
add it with
`flatpak remote-add --if-not-exists --user cosmic https://apt.pop-os.org/cosmic/cosmic.flatpakrepo`
— or via the Store's recommended-sources page (finding 1). Submission mechanics,
verified from the repo (cloned 2026-10-10):

- A PR adds `app/<id>/<id>.json` (+ regenerated `cargo-sources.json`); PR CI builds
  every changed manifest with `flatpak-builder --sandbox --install-deps-from=flathub`
  (`.github/workflows/ci.yml`, justfile `build`/`build-changed`). Merged manifests
  are rebuilt into a GPG-signed OSTree repo (`just repo` runs
  `flatpak build-update-repo --gpg-sign --generate-static-deltas`) and published
  under `apt.pop-os.org/cosmic/` (scripts/rsync-repos). UNVERIFIED: the exact
  internal trigger that runs the publish after merge — only PR CI is visible in-repo.
- The enforced PR checklist includes: COSMIC trademark policy read and followed; no
  LLM-generated content beyond your own app code/manifest; DCO; tested and working
  (`.github/PULL_REQUEST_TEMPLATE.md`, enforced by
  `.github/workflows/require-checklist.yml`).
- Reference applet manifest, `dev.cappsy.CosmicExtAppletDict.json`: runtime
  `org.freedesktop.Platform 25.08`, rust-stable SDK extension, base
  `com.system76.Cosmic.BaseApp` branch `stable`, finish-args
  `--socket=wayland --socket=fallback-x11 --share=ipc
  --talk-name=com.system76.CosmicSettingsDaemon --talk-name=org.freedesktop.Flatpak
  --filesystem=xdg-config/cosmic:rw`, build via `cargo --offline fetch/build
  --features flatpak` from a git tag + cargo-sources.json, installing the desktop
  file as `/app/share/applications/<appid>.desktop`, metainfo, and hicolor icon.
  (UNVERIFIED: what `com.system76.Cosmic.BaseApp` actually prebuilds — its manifest
  is not in the cosmic-flatpak repo; five applet manifests depend on it and
  `cosmic.flatpakrepo` requires Flathub, implying it layers COSMIC bits on the FDW
  runtime.)

### 3. Native packaging and the System76 release convention

#### 3.1 cargo-deb

[cargo-deb](https://github.com/kornelski/cargo-deb) produces a binary `.deb` from
Cargo metadata: `cargo deb` → `target/debian/<name>_<version>-1_<arch>.deb`. Zero
config gets a working package from `name/version/license/description/repository` in
`Cargo.toml`; `[package.metadata.deb]` adds `maintainer`, `depends`, `revision`,
`section`, and — the important one — `assets = [[source, dest, mode]…]` with globs,
plus `[package.metadata.deb.variants.<name>]` for multiple debs from one crate and
automatic `$auto` deps. Binaries are stripped by default; systemd units are
first-class (`[package.metadata.deb.systemd-units]`)
([cargo-deb README](https://github.com/kornelski/cargo-deb/blob/main/README.md)).
For DropTerm the deb shape is simple precisely because the applet is a **binary, not
a .so** — out-of-process applets need nothing in a special lib path. One deb with:

```
usr/bin/cosmic-ext-dropterm            ← cargo-deb auto-detects workspace bins, or list as assets
usr/bin/cosmic-ext-dropterm-applet
usr/share/applications/io.github.l1br3_io.CosmicExtDropterm.desktop
usr/share/applications/io.github.l1br3_io.CosmicExtDropterm.Applet.desktop
usr/share/metainfo/io.github.l1br3_io.CosmicExtDropterm.metainfo.xml
usr/share/icons/hicolor/…              ← res/icons content
```

That matches exactly where cosmic-panel/cosmic-settings look (XDG data dirs + PATH —
finding 2.4) and what the justfile `install` convention does upstream. Note
cargo-deb packages ONE package per invocation — for a workspace, either run it on
the root package with explicit assets for the applet bin (simplest, one deb), or
build two debs via variants/metadata in each member. (The coordinator's correction
is baked in here: there is no `.so`/libloading model anywhere in current cosmic-panel
— applets are processes since the `spawn_clients` redesign.)

#### 3.2 What a "COSMIC release" looks like at pop-os (read from cosmic-term)

Read from [pop-os/cosmic-term](https://github.com/pop-os/cosmic-term) @ master
(cloned 2026-10-10), cross-checked with cosmic-settings and cosmic-applets:

- **No GitHub Releases, no release workflow.** `.github/workflows/` contains only
  `rust-checks.yml`, `validate-desktop-files.yml`, `require-checklist.yml`, and
  `flakehub-publish-rolling.yml` (which pushes a rolling flake to FlakeHub on every
  master push). `gh api repos/pop-os/cosmic-term/releases` returns an empty list —
  the answer to "tag → GitHub Release, tarball + sha256" is: **pop-os does not do
  that**.
- **The justfile is the release artifact factory.** `just vendor` runs
  `cargo vendor --sync Cargo.toml`, writes `.cargo/config.toml` pointing at
  `vendor/`, stamps `VERGEN_GIT_COMMIT_DATE`/`VERGEN_GIT_SHA` from
  SOURCE_DATE_EPOCH/SOURCE_GIT_HASH, and tars it all into `vendor.tar`;
  `build-vendored` = `vendor-extract` + `cargo build --release --frozen --offline`;
  `install` installs bin + desktop + metainfo + icons under `rootdir/prefix` (all
  paths verified in the justfile).
- **The deb is built from an in-repo `debian/` directory.** cosmic-term (and
  cosmic-settings, cosmic-applets — all identical pattern) ship `debian/{changelog,
  control, rules, install, copyright}`: `debian/rules` is `dh $@` with
  `override_dh_auto_build: test -e vendor.tar && just build-vendored || just
  build-release` and `override_dh_auto_install: just rootdir=$(DESTDIR) install`.
- **Versioning is the "epoch" lockstep.** Every COSMIC repo bumps together: git tag
  `epoch-X.Y.0` (annotated, e.g. `epoch-1.9.0` tagger Jeremy Soller; cosmic-term is
  at epoch-1.9.0 with Cargo `version = "1.10.0"` already bumped for the next epoch),
  Cargo.toml version, and a `debian/changelog` entry `(1.10.0) noble; *
  Epoch 1.10.0 version update` committed the same day (2026-10-05).
- **Users get the deb from Pop's apt repo**, not GitHub — which is why no
  tarball/sha256 assets exist upstream. The `vendor.tar` convention exists so the
  distro build (and anyone else) can build `--frozen --offline`.

For a third-party single maintainer, the transferable parts are: the
vendor/build-vendored/install justfile trio, `epoch`-style lockstep version bumps
(or plain semver), annotated tags, and desktop-file validation in CI
(`desktop-file-validate` on every `.desktop` — `validate-desktop-files.yml`).

#### 3.3 The third-party ecosystem and how community apps actually distribute

- **Hub**: not an "awesome-cosmic" repo — searches find no notable one; the de
  facto hub is the [cosmic-utils org](https://github.com/cosmic-utils) and its
  catalog [cosmic-project-collection](https://github.com/cosmic-utils/cosmic-project-collection)
  (373★, lists ~40 apps + ~30 applets with screenshots; PR-addable).
- **Naming**: the `cosmic-ext-` prefix is System76's *official recommendation* for
  third parties — TRADEMARK.md §5.2: "Third-party developers are encouraged to use
  the 'cosmic-ext-' package namespace. These applications and applets may be
  described as 'for the COSMIC™ desktop'"; `com.system76.` app IDs and the plain
  `cosmic-` namespace are reserved for official software
  ([pop-os/cosmic-epoch TRADEMARK.md](https://github.com/pop-os/cosmic-epoch/blob/master/TRADEMARK.md)).
  DropTerm already complies (`cosmic-ext-dropterm`, `io.github.l1br3_io.*`, "for the
  COSMIC desktop" description) — only the ™ marker is missing.
- **Real distribution patterns** (checked 2026-10-10):
  - **Tweaks** (cosmic-utils/tweaks, 204★): Flathub `dev.edfloreshz.CosmicTweaks`
    + AUR `cosmic-ext-tweaks` (0.2.5) and `-git` variant. Flathub-first.
  - **Tasks** (edfloreshz/tasks, 124★): README leads with the Flathub badge
    (`flathub.org/apps/dev.edfloreshz.Tasks`), `sudo just install` as fallback.
  - **Examine** (cosmic-utils/examine, 25★): Flathub
    `io.github.cosmic_utils.Examine` (runtime 25.08).
  - **Applets** (clipboard-manager 236★, minimon 131★, logomenu): pop-os/cosmic-flatpak
    + AUR (`cosmic-ext-applet-minimon` 1.2.0 + `-git`, `cosmic-ext-applet-logomenu`
    0.8.0 + `-git`) + nixpkgs. Store-repo-first, AUR second.
  - **Direct competitor**: [m0rf30/cosmic-ext-quake-terminal](https://github.com/m0rf30/cosmic-ext-quake-terminal)
    (v0.2.0, GPL-3.0-only) — "Runs as a background daemon and toggles a terminal
    emulator's visibility… uses `zcosmic_toplevel_manager_v1`". Different design
    (wraps another terminal rather than embedding alacritty_terminal), but it
    proves the niche exists and its AUR package `cosmic-ext-quake-terminal` (see
    §4) is the exact template a DropTerm AUR package should follow.

### 4. AUR: conventions, git-dep handling, and the dropterm-shaped gap

Searches via the AUR RPC API (2026-10-10):

- **No `dropterm` package exists** (search "dropterm" → 0 results). The niche is
  open; `cosmic-ext-quake-terminal` 0.2.0-1 is the closest existing package.
- The `cosmic-ext-*` family is well established (~40 packages, `-git` and release
  variants side by side; maintainers include AUR devs like `smiley`).

The two archetypes a DropTerm package should copy:

`cosmic-ext-quake-terminal` (release, upstream tarball):
```sh
source=("${pkgname}-${pkgver}.tar.gz::${url}/archive/${pkgver}.tar.gz")
sha256sums=('…')
prepare() { cd "${pkgname}-${pkgver}"; cargo fetch --locked; }
build()    { just build-release --frozen; }          # with lld link args
package()  { just rootdir="${pkgdir}" install; }
```
([AUR cosmic-ext-quake-terminal](https://aur.archlinux.org/cgit/aur.git/plain/PKGBUILD?h=cosmic-ext-quake-terminal))

`cosmic-ext-applet-logomenu` (release, applet):
```sh
depends=('cosmic-applets'); makedepends=('cargo' 'just' 'git')
prepare() { cargo fetch --target "$(rustc -vV | sed -n 's/host: //p')"; }
build()   { just build-release --frozen; }
package() { just rootdir="$pkgdir" install; }
```
([AUR cosmic-ext-applet-logomenu](https://aur.archlinux.org/cgit/aur.git/plain/PKGBUILD?h=cosmic-ext-applet-logomenu))

The **checksum policy**: only the top-level release tarball is hashed
(sha256 of the GitHub archive); the libcosmic git dependency is pinned by
`Cargo.lock` (committed upstream — AUR's cargo guidelines require a locked,
committed Cargo.lock) and fetched at build time via `cargo fetch --locked` in
`prepare()`; the build then runs `--frozen`. This works because makepkg builds are
not network-isolated by default. `-git` packages instead use `git+<url>#branch=…`
sources and `pkgver()` from `git describe`. Naming: package name = upstream repo
name (`cosmic-ext-dropterm`), optional `-git` twin. DropTerm's justfile already
speaks `rootdir=` so a PKGBUILD is nearly the logomenu one verbatim — plus the
`applet` workspace member must be built too (`cargo build -p cosmic-ext-dropterm
-p cosmic-ext-dropterm-applet --frozen` or a `just build-release --workspace`).

### 5. Nix: flake now, nixpkgs later — git libcosmic deps are a solved problem both ways

nixpkgs has **first-class COSMIC packaging**: a dedicated manual chapter documents
`libcosmicAppHook`, which "wraps every executable, sets the search paths libcosmic
reads at runtime, and links the libraries that libcosmic and Iced load with
dlopen", injects `cosmic-settings` theme fallbacks and `cosmic-icons` into
`XDG_DATA_DIRS`, and handles Vergen env quirks
([nixpkgs cosmic.section.md](https://github.com/NixOS/nixpkgs/blob/master/doc/languages-frameworks/cosmic.section.md)).

- **Existing cosmic-ext packages in nixpkgs** (searched 2026-10-10):
  `cosmic-ext-tweaks`, `cosmic-ext-calculator`, `cosmic-ext-applet-sysinfo`,
  `cosmic-ext-applet-weather`, `cosmic-ext-applet-minimon`,
  `cosmic-ext-applet-caffeine`, `cosmic-ext-applet-mare-player`,
  `cosmic-ext-applet-workspace-icons`, `cosmic-ext-applet-privacy-indicator`,
  `cosmic-ext-applet-external-monitor-brightness`, `cosmic-ext-ctl` — all under
  `pkgs/by-name/co/…`.
- **How they handle the git libcosmic dep**: plain `rustPlatform.buildRustPackage`
  with `src = fetchFromGitHub { rev/tag, hash }` and a single
  `cargoHash = "sha256-…"` — e.g.
  [cosmic-ext-applet-sysinfo/package.nix](https://github.com/NixOS/nixpkgs/blob/master/pkgs/by-name/co/cosmic-ext-applet-sysinfo/package.nix)
  (which depends on git libcosmic upstream) carries no per-git-dep hashes: the
  `cargoHash` fixed-output derivation vendors **all** crate sources including git
  ones ("`buildRustPackage` requires a `cargoHash` attribute, computed over all
  crate sources of this package" —
  [nixpkgs rust.section.md](https://github.com/NixOS/nixpkgs/blob/master/doc/languages-frameworks/rust.section.md)).
- **The `cargoLock` alternative** (used outside nixpkgs and in flakes) is where git
  deps need explicit hashes: "The output hash of each dependency that uses a git
  source must be specified in the `outputHashes` attribute … If you do not specify
  an output hash for a git dependency, building the package will fail and inform
  you of which crate needs to be added" (stub with `lib.fakeHash`, rebuild, read
  the correct hash). For flakes there is an escape hatch: "For usage outside
  nixpkgs, `allowBuiltinFetchGit` could be used to avoid having to specify
  `outputHashes`" (same manual section).

So: a DropTerm **flake** is `buildRustPackage + cargoLock.lockFile +
outputHashes.libcosmic (or allowBuiltinFetchGit) + libcosmicAppHook`-equivalent
wrapping (the hook itself is available on NixOS from nixpkgs even in a flake via
`pkgs.libcosmicAppHook`)`. Getting **into nixpkgs** later means switching to
`fetchFromGitHub` + `cargoHash` against a tag, exactly like sysinfo — a small,
well-trodden PR. Worth noting: pop-os repos publish rolling flakes to FlakeHub
(`flakehub-publish-rolling.yml` in cosmic-term) as their nix story; that is a third
option with zero review overhead.

### 6. Release automation: cargo-dist is healthy, but wrong-shaped for COSMIC; pop-os uses none

- **[cargo-dist](https://github.com/axodotdev/cargo-dist)** (now just `dist`):
  actively maintained — v0.33.0 released 2026-09-11. Flow: `cargo dist init`
  generates a `release.yml` GitHub workflow implementing "plan (waits for a pushed
  v-tag), build (per-platform matrix), publish (package managers), host + announce
  (GitHub Release with artifacts and changelog)" — tag push is the only human
  action ([README](https://github.com/axodotdev/cargo-dist/blob/main/README.md)).
  **Current installer set: shell, powershell, npm, homebrew, msi — and no deb**;
  the explicitly *unimplemented* wishlist includes "linux flatpak" and docker
  ([dist book — installers](https://axodotdev.github.io/cargo-dist/book/installers/index.html)).
  (The "deb/msi packages" capability in the task brief is half right: msi yes, deb
  no.) A git libcosmic dep is a non-issue: dist's CI does ordinary `cargo build` in
  a networked checkout, no vendoring required.
- **What pop-os actually uses**: nothing from the above. Verified across
  cosmic-term/edit/settings/applets/store clones — no release tooling at all;
  releases are the epoch-tag + `debian/` + dh pipeline (finding 3.2), plus
  FlakeHub rolling pushes for nix.
- **Alternatives**: [release-plz](https://github.com/MarcoIeni/release-plz)
  (1498★, pushed 2026-10-10) automates version-bump PRs, changelogs, tags, and
  crates.io publishing from Cargo changes; [cargo-release](https://github.com/crate-ci/cargo-release)
  (1592★, pushed 2026-10-04) is the lighter `cargo release` verb for the same
  steps. Both target crates-centric workflows; DropTerm ships no crate to
  crates.io, so their value is mostly the changelog/tag hygiene.
- **A plain GitHub Actions matrix** (build x86_64 (+aarch64) release binaries,
  `just vendor` + `cargo build --frozen --offline` or plain build, attach
  `vendor.tar`/binary tarballs + `sha256sums` to a GitHub Release) reproduces the
  useful 20% of dist with zero new tooling and artifacts downstream packagers
  actually want (the AUR PKGBUILDs above consume exactly such GitHub tag tarballs
  today — quake-terminal's release tag has no assets, so AUR uses the auto
  `archive/<tag>.tar.gz`, which works but breaks on history rewrites).

### 7. Go-to-market hygiene for v1.0 (checklist, all from primary specs)

- **AppStream metainfo** — Flathub treats warnings as fatal
  (`flatpak-builder-lint` / `appstreamcli validate`; "Both errors and warnings are
  considered to be fatal"
  ([metainfo-guidelines](https://docs.flathub.org/docs/for-app-authors/metainfo-guidelines))).
  Required: `<releases>` ("Applications **must** supply a releases tag"); OARS
  `type="oars-1.1"` ("must be properly tagged by OARS data" — generate at
  [hughsie.github.io/oars](https://hughsie.github.io/oars/)); `<developer><id>` +
  `<name>` (reverse-DNS id); `<metadata_license>` (CC0-1.0) + `project_license`
  (GPL-3.0-only); `<url type="homepage">` (`vcs-browser` recommended); description
  with at least one non-empty `<p>`; `<launchable type="desktop-id">` matching the
  desktop file; install to `/app/share/metainfo/<id>.metainfo.xml` (native:
  `/usr/share/metainfo`). Compare System76's own cosmic-term metainfo — it has
  none of the Flathub musts (no releases, no OARS, no screenshots) because it
  targets only Pop's deb pipeline
  ([cosmic-term res/…metainfo.xml](https://github.com/pop-os/cosmic-term/blob/master/res/com.system76.CosmicTerm.metainfo.xml)).
- **Screenshots** — required ("All graphical applications must have one or more
  screenshots in the MetaInfo"), window-only captures "1000x700 pixels or smaller
  (2000x1400 for HiDPI)", native decorations on (don't maximize), taken on Linux,
  default settings, one-sentence captions without a full stop, 3-6 shots is the
  sweet spot, `type="default"` on the lead shot, image URLs from a tag/commit not
  a branch ([quality-guidelines](https://docs.flathub.org/docs/for-app-authors/metainfo-guidelines/quality-guidelines)).
- **Name/summary quality** — name ideally ≤15 chars ("DropTerm" ✓), summary
  10-35 chars, sentence case, no toolkit words, imperative verbs (e.g. "Drop-down
  terminal for the COSMIC desktop" is slightly long but in range; avoid repeating
  the name) (same page).
- **Desktop file** — `desktop-file-validate` in CI exactly like pop-os's
  `validate-desktop-files.yml`; `StartupNotify=true` (cosmic-term sets it);
  `StartupWMClass` per the
  [desktop-entry spec recognized-keys](https://specifications.freedesktop.org/desktop-entry-spec/latest/recognized-keys.html)
  should match the binary's WM class under X11/XWayland — set
  `StartupWMClass=cosmic-ext-dropterm`; on Wayland the `app_id` (which libcosmic
  sets from APP_ID) is what COSMIC matches, so the key is hygiene, not function.
  Note cosmic-term uses a nonstandard `Categories=COSMIC;System;TerminalEmulator;`
  — community apps on Flathub stick to registered categories (`System;
  TerminalEmulator` as DropTerm already does).
- **Versioning** — two visible conventions: System76's epoch semver (`epoch-X.Y.0`,
  bump Cargo + tag + changelog together); community mostly plain semver 0.x
  (tweaks 0.2.5, quake-terminal 0.2.0, logomenu 0.8.0) with occasional CalVer
  (cosmicding `v2025.11.1`). For DropTerm: plain semver, `v<version>` annotated
  tags, no CalVer needed.
- **Signing** — nobody signs git tags in this ecosystem (pop-os epoch tags are
  annotated, not evidently signed); what IS signed is the cosmic-flatpak OSTree
  repo (Pop's ISO signing key, `--gpg-sign` in its justfile) and Flathub's own
  repo — i.e., signing is the distributor's job, not the app author's. GitHub
  Release artifacts + sha256 sidecar is the AUR-grade bar.

### 8. Terminal-specific gotchas

#### 8.1 PTY/shell spawn inside the Flatpak sandbox

Yes — left alone, the shell users get is the **runtime's** shell: a Flatpak app
spawning `$SHELL`/`bash` executes the Freedesktop runtime's `/usr/bin/bash` against
the sandbox filesystem, where the host's home is invisible (except granted
`--filesystem=` subsets) and host binaries dotfiles and tools don't exist. The
platform-blessed fix, used by the reference terminal Flatpak
([app.devsuite.Ptyxis](https://flathub.org/apps/app.devsuite.Ptyxis)): grant
`--talk-name=org.freedesktop.Flatpak` and run host processes via
`flatpak-spawn --host`; Ptyxis goes further with a dedicated `ptyxis-agent`
"typically running on the host system (even when the Ptyxis UI is a Flatpak)",
falling back to an in-sandbox agent when the host spawn fails — "When Ptyxis UI is
a Flatpak, it uses permissions like `org.freedesktop.Flatpak` (for
`flatpak-spawn --host`) to access the host for its agent"
([Ptyxis README](https://gitlab.gnome.org/chergert/ptyxis/-/blob/main/README.md));
its manifest adds `--filesystem=host` + `--talk-name=org.freedesktop.Flatpak` on
top of the usual wayland/dri/ipc sockets
([flathub/app.devsuite.Ptyxis manifest](https://github.com/flathub/app.devsuite.Ptyxis/blob/master/app.devsuite.Ptyxis.json)).
For DropTerm this is the killer detail on top of §2.4: even if layer-shell were
solved, every PTY DropTerm spawns would be runtime-context shells unless it grew a
Ptyxis-style host-agent — a large behavioral difference from the native build.
(All moot for the main app as long as cosmic-comp gates layer-shell.)

#### 8.2 D-Bus across the sandbox boundary (host applet ↔ flatpaked app, and the reverse)

Flatpak filters the session bus: by default an app may own exactly
`$FLATPAK_ID`, its subnames, and `org.mpris.MediaPlayer2.$FLATPAK_ID`, and may
talk to those plus `org.freedesktop.DBus` and `org.freedesktop.portal.*`;
anything more needs `--own-name`/`--talk-name`, and
`--socket=session-bus` (unfiltered) "is a security risk"
([flatpak sandbox permissions](https://docs.flatpak.org/en/latest/sandbox-permissions.html)).
Consequences for DropTerm's two D-Bus relationships:

- **Flatpaked main app**: libcosmic's single-instance name IS the APP_ID = the
  Flatpak ID (`io.github.l1br3_io.CosmicExtDropterm`), so the app can own it by
  default with no extra finish-args. A host-side caller (the native applet, the
  host `cosmic-ext-dropterm toggle` CLI) reaches it through the per-app D-Bus
  proxy — the same direction host DEs use to control sandboxed MPRIS players, so
  own-name implies reachable-ness in practice (the docs specify only the
  sandbox-side rules; the host→sandbox direction is evidenced by the MPRIS
  default-allow, UNVERIFIED as an explicit statement in docs).
- **Flatpaked applet (the realistic mix)**: the applet must call the
  *host-owned* name `io.github.l1br3_io.CosmicExtDropterm` — that is an outbound
  talk, so the applet manifest needs
  `--talk-name=io.github.l1br3_io.CosmicExtDropterm`. Its `NameOwnerChanged`
  watch (running/not state) rides the same grant. The reverse mix (host applet ↔
  flatpaked app) is the case above.
- **Both native** (the recommended baseline): no boundary, plain session-bus
  behavior as DropTerm has today.

#### 8.3 Notifications inside Flatpak

DropTerm's bell uses `notify-rust` → `org.freedesktop.Notifications` directly
(COSMIC has no portal consumer path wired in DropTerm today). In a Flatpak that
name is not talkable by default; two options: `--talk-name=org.freedesktop.Notifications`
(the permissive-looking but common choice) or switch to the portal interface
`org.freedesktop.portal.Notification` (`AddNotification`), which is in the
default-allowed portal namespace — its D-Bus definition ships in
[flatpak/xdg-desktop-portal `data/org.freedesktop.portal.Notification.xml`](https://github.com/flatpak/xdg-desktop-portal/blob/main/data/org.freedesktop.portal.Notification.xml).
The portal is fire-and-forget (no actions/timeout control like the desktop
notification spec), which matters for a "bell attention" UX. For the applet in
cosmic-flatpak, whichever path is chosen just needs the matching finish-arg.

## Recommended path

Ranking for THIS project — a layer-shell drop-down terminal + out-of-process panel
applet, single maintainer, appstore-style reach desired — by (reach × fit) / effort:

1. **Native release spine (do first, everything hangs off it).** Adopt the
   System76 convention: extend the justfile with `vendor`/`build-vendored` +
   `rootdir=` install covering BOTH bins, desktop files, metainfo, icons; CI runs
   `desktop-file-validate` + tests; release = semver bump + annotated `v<version>`
   tag + GitHub Release with source archive, `vendor.tar`, per-arch binary tarballs
   and a `sha256sums` file (plain GHA matrix; cargo-dist optional but its deb gap
   and COSMIC-specific needs make it skippable). Effort S; enables items 2-4.
2. **Applet → pop-os/cosmic-flatpak (the single highest-leverage submission).**
   Store-visible to every COSMIC user via the built-in "COSMIC Flatpak" source;
   the platform's flatpak applet path is built for exactly this shape; manifest is
   ~40 lines on the dict-applet template with
   `--talk-name=io.github.l1br3_io.CosmicExtDropterm` added for the host-app
   control channel; PR checklist is satisfiable by a human author (no
   LLM-generated manifest content). Effort S-M. Caveat to test in CI/locally:
   applet-in-sandbox toggling the NATIVE app across the bus boundary (§8.2).
3. **AUR `cosmic-ext-dropterm` (+ optional `-git`).** The niche is empty
   (no dropterm package exists; the quake-terminal PKGBUILD is a copy-adapt);
   Arch/COSMIC-derivative users are the natural early adopters. Effort S.
4. **cargo-deb deb from the GitHub release.** Pop!_OS users who never leave apt:
   double-click install via cosmic-store; also the artifact to hand to anyone
   wanting a PPA. Effort S. (No apt repo of your own — not worth it.)
5. **Nix flake in-repo, nixpkgs PR when stable.** Flake with
   `cargoLock` + `outputHashes` (or `allowBuiltinFetchGit`) + wrapping à la
   `libcosmicAppHook`; nixpkgs PR later copies the sysinfo shape (`fetchFromGitHub`
   + `cargoHash`) once there is a tagged release history. Effort M, mostly waiting
   on review.
6. **Flathub for the main app — blocked, revisit only on a trigger.** The
   layer-shell sandbox gate (§2.4) makes the flagship feature impossible inside a
   standard Flatpak on cosmic-comp today; revisit only if (a) cosmic-comp grows an
   intended sandboxed-layer-shell path, or (b) DropTerm grows a non-layer-shell
   windowed fallback mode worth shipping separately. Do NOT ship a Flatpak that
   silently can't drop down — that app-switcher lesson is the loudest one in this
   research.
7. **Ecosystem hygiene (cheap, compounding):** add the ™ ("for the COSMIC™
   desktop") per TRADEMARK.md §5.2, PR into
   cosmic-utils/cosmic-project-collection, Flathub-grade metainfo (releases, OARS,
   ≤1000×700 screenshots) even for the native/deb path so cosmic-store renders it
   well, and `StartupWMClass=cosmic-ext-dropterm` in the desktop file.

## Sources

- cosmic-store @ master (cloned 2026-10-10): `src/backend/mod.rs` (BackendName
  enum); `src/main.rs:1895-1935` (Flathub + "COSMIC Flatpak" recommended sources,
  `res/flathub.flatpakrepo`, `res/cosmic.flatpakrepo` → `https://apt.pop-os.org/cosmic/`);
  `src/priority.rs:25-26`; `src/view.rs:1219` — <https://github.com/pop-os/cosmic-store>
- System76 support, "Managing apps" (fetched 2026-10-10): Flathub + Ubuntu repos
  default in COSMIC Store, .deb double-click install, Repoman —
  <https://system76.com/support/managing-apps>
- libcosmic README @ master (fetched 2026-10-10): Flathub-vs-COSMIC-Flatpak routing,
  trademark-policy pointer —
  <https://github.com/pop-os/libcosmic/blob/master/README.md>
- cosmic-panel @ `3ce6ce1` (2026-10-06, cloned 2026-10-10):
  `cosmic-panel-bin/src/space/wrapper_space.rs:375-520` (desktop-file stem
  discovery), `:540-633` (env/fds, `is_flatpak` arg injection:
  `--socket=inherit-wayland-socket`, `--die-with-parent`, `--env=`), `:760-800`
  (respawn socket refresh);
  `cosmic-panel-bin/src/space/panel_space.rs:203-209` (`PathSource::guess_from`) —
  <https://github.com/pop-os/cosmic-panel>
- cosmic-settings @ master (cloned 2026-10-10):
  `cosmic-settings/src/pages/desktop/panel/applets_inner.rs:71,571`;
  `debian/{rules,changelog}` — <https://github.com/pop-os/cosmic-settings>
- pop-os/freedesktop-desktop-entry `src/lib.rs:678-685` (default_paths = XDG data
  dirs) — <https://github.com/pop-os/freedesktop-desktop-entry>
- flatpak @ main (fetched 2026-10-10): `profile/flatpak.sh`, `env.d/flatpak.env.in`
  (XDG_DATA_DIRS export dirs) — <https://github.com/flatpak/flatpak>
- flatpak docs (fetched 2026-10-10): sandbox permissions (D-Bus default allowlist,
  sockets, inherit-wayland-socket sensitivity) —
  <https://docs.flatpak.org/en/latest/sandbox-permissions.html>;
  conventions (desktop-file Exec rewrite, export paths, app-ID rules) —
  <https://docs.flatpak.org/en/latest/conventions.html>
- cosmic-comp @ `4d14528` (2026-10-08, cloned 2026-10-10):
  `src/state.rs:165-174` (`not_sandboxed`, `com.system76.CosmicPanel` engine
  carve-out), `:649-652`, `:738` (layer-shell filter) —
  <https://github.com/pop-os/cosmic-comp>
- pop-os/cosmic-flatpak (cloned 2026-10-10; 56 applets): README; justfile
  (`build`, `repo`, `eol`, `build-update-repo --gpg-sign`); `.github/workflows/{ci,require-checklist}.yml`;
  `.github/PULL_REQUEST_TEMPLATE.md`;
  `app/dev.cappsy.CosmicExtAppletDict/{dev.cappsy.CosmicExtAppletDict.json,cargo-sources.json}` —
  <https://github.com/pop-os/cosmic-flatpak>
- cosmic-flatpak issues #259 (app-switcher removal: security-context/layer-shell
  measurement, `--filesystem=/usr` reserved), #213, #268 (withdraw flow), #331-348
  (submission cadence) — <https://github.com/pop-os/cosmic-flatpak/issues>
- cosmic-term @ master (cloned 2026-10-10): justfile (vendor/vendor.tar/
  build-vendored/install); `res/com.system76.CosmicTerm.metainfo.xml`;
  `res/com.system76.CosmicTerm.desktop` (Categories=COSMIC; StartupNotify);
  `.github/workflows/{rust-checks,validate-desktop-files,flakehub-publish-rolling,require-checklist}.yml`;
  tags `epoch-1.x.0` (annotated); `debian/` (same pattern verified in
  cosmic-settings and cosmic-applets); `gh api repos/pop-os/cosmic-term/releases` →
  empty — <https://github.com/pop-os/cosmic-term>
- Flathub (fetched 2026-10-10):
  <https://flathub.org/apps/com.system76.CosmicTerm> (404);
  AppStream index `dl.flathub.org/repo/appstream/x86_64/appstream.xml.gz` (COSMIC
  apps present: CosmicTweaks, Tasks, Examine, cosmicding, camera, color-picker…);
  docs: submission, requirements, metainfo-guidelines + quality-guidelines,
  github-actions — <https://docs.flathub.org/docs/for-app-authors/submission> etc.;
  flathub org repo creation stream (`gh api orgs/flathub/repos?sort=created`) and
  `repos/flathub/flathub/branches/new-pr`; manifests:
  <https://github.com/flathub/com.vkhitrin.cosmicding/blob/master/com.vkhitrin.cosmicding.yaml>,
  <https://github.com/flathub/io.github.cosmic_utils.Examine/blob/master/io.github.cosmic_utils.Examine.json>,
  <https://github.com/flathub/app.devsuite.Ptyxis/blob/master/app.devsuite.Ptyxis.json>
- flatpak-builder-tools cargo generator README (usage, CARGO_HOME, offline) —
  <https://github.com/flatpak/flatpak-builder-tools/tree/master/cargo>
- cargo-deb README (assets, variants, systemd-units, stripping) —
  <https://github.com/kornelski/cargo-deb/blob/main/README.md>
- AUR RPC + cgit (fetched 2026-10-10): searches `cosmic-ext` (~40 hits),
  `dropterm` (0 hits); PKGBUILDs of `cosmic-ext-quake-terminal` and
  `cosmic-ext-applet-logomenu` —
  <https://aur.archlinux.org/cgit/aur.git/plain/PKGBUILD?h=cosmic-ext-quake-terminal>,
  <https://aur.archlinux.org/cgit/aur.git/plain/PKGBUILD?h=cosmic-ext-applet-logomenu>
- NixOS/nixpkgs @ master (fetched 2026-10-10):
  `doc/languages-frameworks/cosmic.section.md` (libcosmicAppHook);
  `doc/languages-frameworks/rust.section.md` (cargoHash over all crate sources,
  outputHashes for git deps, allowBuiltinFetchGit);
  `pkgs/by-name/co/cosmic-ext-applet-sysinfo/package.nix` (fetchFromGitHub +
  cargoHash + libcosmicAppHook pattern; 11 `cosmic-ext-*` packages found) —
  <https://github.com/NixOS/nixpkgs>
- cargo-dist: repo README (plan/build/host/publish/announce; tag-driven
  release.yml) — <https://github.com/axodotdev/cargo-dist>; v0.33.0 (2026-09-11,
  `gh api repos/axodotdev/cargo-dist/releases`); installer set incl. NO deb and
  flatpak-on-wishlist —
  <https://axodotdev.github.io/cargo-dist/book/installers/index.html>;
  release-plz <https://github.com/MarcoIeni/release-plz> (1498★, active);
  cargo-release <https://github.com/crate-ci/cargo-release> (1592★, active)
- cosmic-utils org + cosmic-project-collection (fetched 2026-10-10; star counts via
  GitHub API) — <https://github.com/cosmic-utils>,
  <https://github.com/cosmic-utils/cosmic-project-collection>;
  Tasks README (Flathub badge) — <https://github.com/edfloreshz/tasks>
- m0rf30/cosmic-ext-quake-terminal @ v0.2.0 (cloned 2026-10-10): README (daemon +
  toplevel-manager design), justfile, tags 0.1.0/0.2.0, release 0.2.0 with no
  assets — <https://github.com/m0rf30/cosmic-ext-quake-terminal>
- pop-os/cosmic-epoch TRADEMARK.md §5 (cosmic-ext- namespace, com.system76.
  reserved, "for the COSMIC™ desktop") —
  <https://github.com/pop-os/cosmic-epoch/blob/master/TRADEMARK.md>
- Ptyxis README @ main (fetched 2026-10-10): ptyxis-agent on host, flatpak-spawn
  --host fallback — <https://gitlab.gnome.org/chergert/ptyxis/-/blob/main/README.md>
- xdg-desktop-portal `data/org.freedesktop.portal.Notification.xml` (portal
  notification API exists) —
  <https://github.com/flatpak/xdg-desktop-portal/blob/main/data/org.freedesktop.portal.Notification.xml>
- Desktop Entry Spec, recognized keys (StartupWMClass) —
  <https://specifications.freedesktop.org/desktop-entry-spec/latest/recognized-keys.html>;
  OARS generator — <https://hughsie.github.io/oars/>
- DropTerm itself: `Cargo.toml` (workspace, pinned libcosmic rev `08a09e3`,
  GPL-3.0-only, bins), `applet/Cargo.toml`, `res/io.github.l1br3_io.CosmicExtDropterm{,.Applet}.desktop`,
  `src/surface.rs` (layer-surface construction)
