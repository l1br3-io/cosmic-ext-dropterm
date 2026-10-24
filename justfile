# Recipes are silent on success and announce only on failure. `set quiet`
# stops just from echoing each recipe line; a failing recipe still prints
# just's `error: Recipe ... failed` line plus the underlying tool's stderr.
set quiet

name := 'cosmic-ext-dropterm'
appid := 'io.github.l1br3_io.CosmicExtDropterm'

rootdir := ''
prefix := '/usr'

base-dir := absolute_path(clean(rootdir / prefix))

# Absolute by default (justfile_directory, not cwd, so a `just`
# invoked from a subdirectory still resolves): `dev-install`
# symlinks `dev-image` into `~/.local/bin`, and a relative symlink
# target would resolve from the link's directory there — the
# silently-dead-launcher F12 lesson. Every other consumer runs from
# the justfile directory, where absolute paths work identically.
cargo-target-dir := env('CARGO_TARGET_DIR', justfile_directory() / 'target')
bin-src := cargo-target-dir / 'release' / name
bin-dst := base-dir / 'bin' / name
debug-bin := cargo-target-dir / 'debug' / name

# rustup installs cargo into CARGO_HOME/bin, which is not on PATH in
# the shell that first bootstraps it — and recipes each run their own
# shell, so an install by one recipe does not carry to the next.
# Prepending it for every recipe lets the same `just build-all` that
# just installed the toolchain immediately run it. A nonexistent PATH
# entry is harmless: under `sudo just` the home is root's and the
# entry simply never matches anything.
export PATH := env('CARGO_HOME', home_directory() / '.cargo') / 'bin' + ':' + env('PATH', '')

# Where the dev launcher lives. The launcher carries a `-dev` suffix
# so it never shadows an installed release: `cosmic-ext-dropterm` on
# PATH is always the installed copy, `cosmic-ext-dropterm-dev` always
# the dev build.
dev-bin-dir := env('XDG_BIN_HOME', home_directory() / '.local' / 'bin')
dev-bin := dev-bin-dir / (name + '-dev')

# The dev identity's own binary image. The feature-flagged build is
# COPIED here (never symlinked to the shared debug bin): a plain
# `cargo build`/`cargo test` — including inside `just check` —
# rebuilds `target/debug/cosmic-ext-dropterm` WITHOUT the
# `dev-identity` feature, and a launcher pointing at that shared
# path would silently flip to release identity: its next launch
# would activate (and toggle) the installed release instead of the
# dev instance. The image only ever changes through `dev-install` /
# `dev-reload`, so the dev identity cannot be overridden by an
# unrelated build.
dev-image := cargo-target-dir / 'debug' / (name + '-dev-image')

# The dev recipes build the debug binaries re-identified as `.Dev`
# (the dev-identity cargo feature): their own single-instance slot,
# bus name, config file, and session, so a dev cycle never touches
# an installed release's.
dev-identity-args := '--features=dev-identity'

# Show all recipes when `just` is run with no arguments (without a
# default recipe, just runs the first one — dev-image-install —
# silently, under `set quiet`).
default:
    just --list --unsorted

# Installs the freshly built dev-identity debug binary to the dev
# image path. Every consumer of the dev identity (the launcher
# symlink, `dev-reload`, the `act` recipes) runs the image, never
# the shared debug bin.
dev-image-install:
    install -Dm0755 {{debug-bin}} {{dev-image}}

# `pkill -f` pattern matching only the dev instance — the launcher's
# `-dev` name, the dev image's target path, or the debug binary's
# target path with a `( |$)` guard so it stops at the binary name
# instead of also matching the applet's debug path (whose name
# extends cosmic-ext-dropterm with `-applet`). Each alternative's
# first character is bracketed so the pattern cannot match this
# recipe's own `sh -c` command line. The installed release and its
# applet never match — leaving them running through a dev cycle is
# the point of the dev-identity split.
dev-kill-pattern := '[c]osmic-ext-dropterm-dev|[t]arget/debug/cosmic-ext-dropterm-dev-image( |$)|[t]arget/debug/cosmic-ext-dropterm( |$)'

desktop := appid + '.desktop'
desktop-src := 'res' / desktop
desktop-dst := clean(rootdir / prefix) / 'share' / 'applications' / desktop

metainfo := appid + '.metainfo.xml'
metainfo-src := 'res' / metainfo
metainfo-dst := clean(rootdir / prefix) / 'share' / 'metainfo' / metainfo

icon-src := 'res' / 'icons' / 'hicolor' / 'scalable' / 'apps' / (appid + '.svg')
icon-dst := clean(rootdir / prefix) / 'share' / 'icons' / 'hicolor' / 'scalable' / 'apps' / (appid + '.svg')

# The panel applet: its own workspace package so its libcosmic feature
# set never meets the terminal's wgpu renderer.
applet := name + '-applet'
applet-desktop := appid + '.Applet.desktop'
applet-icon := appid + '-symbolic.svg'
applet-bin-src := cargo-target-dir / 'release' / applet
applet-bin-dst := base-dir / 'bin' / applet
applet-desktop-src := 'res' / applet-desktop
applet-desktop-dst := clean(rootdir / prefix) / 'share' / 'applications' / applet-desktop
applet-metainfo := appid + '.Applet.metainfo.xml'
applet-metainfo-src := 'res' / applet-metainfo
applet-metainfo-dst := clean(rootdir / prefix) / 'share' / 'metainfo' / applet-metainfo
applet-icon-src := 'res' / 'icons' / 'hicolor' / 'scalable' / 'apps' / applet-icon
applet-icon-dst := clean(rootdir / prefix) / 'share' / 'icons' / 'hicolor' / 'scalable' / 'apps' / applet-icon

# The applet's flatpak id doubles as its desktop-file stem: the panel
# matches applets by stem, and flatpak exports the desktop file under
# the flatpak id, so the two must be the same string.
applet-appid := appid + '.Applet'
flatpak-dir := 'packaging' / 'flatpak'
applet-manifest := flatpak-dir / (applet-appid + '.json')

# Where dev-applet installs the applet's desktop file and icon.
dev-data-dir := env('XDG_DATA_HOME', home_directory() / '.local' / 'share')

# The dev terminal's desktop file: the res/ original with the dev
# name, Exec, and StartupWMClass, generated here (like the dev
# applet's) so the desktop contract has one source of truth. The stem
# is the dev identity's app id — the window switcher matches the
# clipboard-anchor toplevel (always mapped) by it, and without the
# file the dev anchor lists as "unknown". NoDisplay stays set
# (inherited): the dev build is a keybind app, not an app-grid entry.
dev-desktop := dev-data-dir / 'applications' / (appid + '.Dev.desktop')

# `cargo fmt --check` + `cargo clippy -D warnings` + `cargo test` +
# the docs link gate, all across the workspace. Union features are
# fine here: nothing renders in tests or lints. Release/install
# recipes stay per-package. clippy and test also run each package
# once with `--features dev-identity`: cfg'd-out identity code
# otherwise never compiles and breaks invisibly.
check: fmt-check clippy test docs-check

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --check --all

clippy *args:
    cargo clippy --all-targets --workspace --locked {{args}} -- -D warnings
    cargo clippy --all-targets -p {{name}} --locked --features dev-identity {{args}} -- -D warnings
    cargo clippy --all-targets -p {{applet}} --locked --features dev-identity {{args}} -- -D warnings

test *args:
    cargo test --workspace --locked {{args}}
    cargo test -p {{name}} --locked --features dev-identity {{args}}
    cargo test -p {{applet}} --locked --features dev-identity {{args}}

# Docs drift gate: every internal markdown link — relative path and
# in-file anchor — in README.md, ROADMAP.md, and docs/ must resolve.
# External (http/mailto) links are out of scope. Silent on success;
# failures name the file and the dangling link.
docs-check:
    #!/usr/bin/env python3
    import re
    from pathlib import Path
    files = [
        f
        for f in [Path("README.md"), Path("ROADMAP.md"), *sorted(Path("docs").rglob("*.md"))]
        # A local `just docs-serve` leaves its build output under
        # docs/_site/ — gitignored, and not source to lint.
        if "/_site/" not in f.as_posix()
    ]
    link = re.compile(r"\[[^\]]*\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")
    def anchors(text):
        heads = re.findall(r"^#+\s+(.*)$", text, re.M)
        return {re.sub(r"\s+", "-", re.sub(r"[^\w\s-]", "", h.strip().lower())) for h in heads}
    bad = []
    for f in files:
        text = f.read_text(encoding="utf-8")
        own = anchors(text)
        for target in link.findall(text):
            if "://" in target or target.startswith("mailto:"):
                continue
            path, _, frag = target.partition("#")
            if path:
                resolved = (f.parent / path).resolve()
                if not resolved.is_file():
                    bad.append(f"{f}: missing file {target}")
                    continue
                if frag and frag not in anchors(resolved.read_text(encoding="utf-8")):
                    bad.append(f"{f}: missing anchor {target}")
            elif frag and frag not in own:
                bad.append(f"{f}: missing anchor {target}")
    if bad:
        raise SystemExit("dangling doc links:\n  " + "\n  ".join(bad))

# Live preview of the docs site — the plugin set GitHub Pages builds
# with, via the jekyll/jekyll docker image, so no local Ruby is
# needed. The disk cache stays off: it would otherwise drop
# root-owned files into docs/.
#
# Port discipline: the container runs under a fixed name, so a
# leftover from a run whose terminal died without the Ctrl-C reaching
# docker (docker's --rm never fires, the orphan holds the port) is
# replaced up front — every run starts fresh. Anything ELSE already
# listening on the port can't be replaced, so the serve falls forward
# to the next free port and prints the URL it actually got (jekyll
# reports its in-container address, a lie under a remapped host port).
# `just docs-serve 4377` starts from a chosen port instead of 4000.
docs-serve port='4000':
    #!/usr/bin/env sh
    set -eu
    port='{{port}}'
    if ! command -v docker >/dev/null 2>&1; then
        echo "docs-serve needs docker (there is no local Ruby toolchain)" >&2
        exit 1
    fi
    docker rm -f dropterm-docs >/dev/null 2>&1 || true
    busy() {
        python3 -c 'import socket,sys; sys.exit(socket.socket().connect_ex(("127.0.0.1", int(sys.argv[1]))) != 0)' "$1"
    }
    while busy "$port"; do
        port=$((port + 1))
    done
    echo "serving at http://127.0.0.1:$port/cosmic-ext-dropterm/"
    exec docker run --rm -i --name dropterm-docs -p 127.0.0.1:"$port":4000 \
        -v "{{justfile_directory()}}/docs:/srv/jekyll" \
        jekyll/jekyll:4 \
        sh -c "gem install --no-document jekyll-relative-links jekyll-default-layout jekyll-titles-from-headings jekyll-readme-index jekyll-optional-front-matter >/dev/null && jekyll serve --host 0.0.0.0 --disable-disk-cache"

# Send a CLI action to the running instance, like a COSMIC keybind would:
# `just act toggle`, `just act new-tab --cwd /tmp`. Runs the dev launcher
# (`just dev-install`); the running instance parses the action itself, so
# the same verb drives a dev or a released instance.
act *args:
    {{dev-bin}} {{args}}

# Toggle the overlay (the default action of a bare launch).
toggle:
    {{dev-bin}} toggle

# Show the overlay without toggling.
show:
    {{dev-bin}} show

# Hide the overlay without toggling.
hide:
    {{dev-bin}} hide

# Open a new tab in the running instance, e.g.
# `just new-tab --cwd /tmp` or `just new-tab --cmd htop`.
new-tab *args:
    {{dev-bin}} new-tab {{args}}

# Release-build load caps: one cargo job = one rustc pinning a core
# through LLVM, and the `lto = true` release profile adds a link-time
# pass over the whole merged dependency graph — uncapped on a big
# machine that starves the desktop for the build's duration. The
# caps keep a release build gentle: few enough jobs to leave cores
# free, at the lowest scheduler priority so anything interactive
# preempts instantly. Override per invocation with the env vars —
# `DROPTERM_BUILD_JOBS=32 DROPTERM_BUILD_NICE=0 just build-release`
# is the old full-speed build.
build-jobs := env('DROPTERM_BUILD_JOBS', '8')
build-nice := env('DROPTERM_BUILD_NICE', '19')

# The prerequisite gate. The build recipes and `install` depend on it,
# and `just deps` runs it alone: it checks everything a build needs —
# the C toolchain, pkg-config, the pkg-config modules behind the
# system libraries (the union of the README's and CI's package
# lists), and cargo — and installs only what is missing, through the
# distro's package manager (apt/dnf/pacman/zypper) and, for Rust,
# rustup's minimal stable profile. Check-first on purpose: with
# everything present — the dev machine, CI (which declares its own
# build-deps), packaging chroots — the gate is a silent no-op that
# never touches sudo or the network. Two deliberate limits: `just`
# itself is out of scope (chicken and egg — it is already running),
# and the rustup bootstrap never runs as root — a root invocation
# (`sudo just install`, a `rootdir=` staging install) never runs
# cargo, and a toolchain installed under sudo would sit root-owned
# in /root where the user cannot update it; the user-side build
# recipes bootstrap it for themselves instead.
deps:
    #!/usr/bin/env sh
    set -eu

    # System side: each module is what the build asks pkg-config for,
    # and each maps to a -dev/-devel package in the lists below. A
    # missing pkg-config or cc means the whole toolchain package is
    # needed, not one module.
    missing=""
    if command -v pkg-config >/dev/null 2>&1; then
        for module in xkbcommon wayland-client fontconfig freetype2 expat; do
            pkg-config --exists "$module" 2>/dev/null || missing="$missing $module"
        done
    else
        missing=" pkg-config"
    fi
    command -v cc >/dev/null 2>&1 || missing="$missing cc"

    if [ -n "$missing" ]; then
        echo "deps: missing:$missing — installing via the system package manager"
        if [ "$(id -u)" -eq 0 ]; then
            sudo=""
        elif command -v sudo >/dev/null 2>&1; then
            sudo="sudo"
        else
            echo "deps: not root and sudo not found — install the above by hand and re-run" >&2
            exit 1
        fi
        # $sudo is deliberately unquoted: it must word-split away when
        # empty (already root).
        if command -v apt-get >/dev/null 2>&1; then
            $sudo apt-get update -qq
            $sudo apt-get install -y build-essential pkg-config \
                libxkbcommon-dev libwayland-dev libfontconfig-dev \
                libfreetype-dev libexpat1-dev
        elif command -v dnf >/dev/null 2>&1; then
            $sudo dnf install -y gcc gcc-c++ make pkgconf-pkg-config \
                libxkbcommon-devel wayland-devel fontconfig-devel \
                freetype-devel expat-devel
        elif command -v pacman >/dev/null 2>&1; then
            $sudo pacman -S --needed --noconfirm base-devel pkgconf \
                libxkbcommon wayland fontconfig freetype2 expat
        elif command -v zypper >/dev/null 2>&1; then
            $sudo zypper --non-interactive install gcc gcc-c++ make pkg-config \
                libxkbcommon-devel wayland-devel fontconfig-devel \
                freetype2-devel libexpat-devel
        else
            echo "deps: no supported package manager (apt-get, dnf, pacman, zypper)" >&2
            echo "deps: install the equivalents of:$missing and re-run" >&2
            exit 1
        fi
    fi

    # Rust side (never as root — see the recipe comment above).
    if [ "$(id -u)" -ne 0 ] && ! command -v cargo >/dev/null 2>&1; then
        echo "deps: cargo not found — installing Rust (rustup, stable, minimal profile)"
        if command -v rustup >/dev/null 2>&1; then
            rustup toolchain install stable --profile minimal
            rustup default stable
        else
            curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
                | sh -s -- -y --profile minimal --default-toolchain stable
        fi
    fi

# Compiles with the debug profile. Per-package on purpose — a plain
# `cargo build` now spans the workspace and unifies features across
# both crates (see build-applet for why that must not produce
# release artifacts).
build-debug *args: deps
    cargo build -p {{name}} {{args}}

# Compiles with the release profile, throttled (see build-jobs).
build-release *args: deps
    nice -n {{build-nice}} cargo build -p {{name}} --release -j {{build-jobs}} {{args}}

# Compiles the applet with the release profile, throttled like
# build-release. Per-package on purpose: a unified workspace release
# build would hand the applet a wgpu-enabled libcosmic through
# feature unification, and the wgpu renderer falls back to llvmpipe
# on panel surfaces.
build-applet *args: deps
    nice -n {{build-nice}} cargo build -p {{applet}} --release -j {{build-jobs}} {{args}}

# Both release artifacts, each per-package, in the shape `install`
# expects. Exists as one recipe because `build-release` takes
# arguments, so `just build-release build-applet` would pass the
# second name through to cargo.
build-all: build-release build-applet

# Vendored offline builds (the System76 release convention): `vendor`
# snapshots the whole dependency graph — including the git-pinned
# libcosmic — into vendor.tar together with the source-replacement
# config cargo vendor prints (a git dependency needs its own
# [source."url"] redirect, so the config cannot be hand-written). The
# working tree is cleaned afterward so dev builds keep using git deps.
vendor:
    mkdir -p .cargo
    cargo vendor --locked | head -n -1 > .cargo/config.toml
    echo 'directory = "vendor"' >> .cargo/config.toml
    tar -cf vendor.tar vendor .cargo/config.toml
    rm -rf vendor .cargo

# Extracts vendor/ and .cargo/config.toml from vendor.tar (see vendor).
vendor-extract:
    rm -rf vendor .cargo
    tar -xf vendor.tar

# Compiles both release artifacts from vendor.tar, per package,
# --frozen --offline: the proof that vendor.tar is a complete
# dependency snapshot (see build-applet for why per-package).
build-vendored *args: vendor-extract (build-release '--frozen' '--offline' args) (build-applet '--frozen' '--offline')

run *args:
    cargo run {{args}}

# Symlinks the dev launcher at `~/.local/bin/cosmic-ext-dropterm-dev`
# to the dev image (see `dev-image-install`) so a COSMIC shortcut
# (and the act recipes) can run it; the `-dev` name leaves an
# installed release's plain name untouched. Also installs the dev
# identity's desktop file (see `dev-desktop`).
dev-install: (build-debug dev-identity-args) dev-image-install
    install -d {{dev-bin-dir}}
    ln -sfn {{dev-image}} {{dev-bin}}
    install -d {{dev-data-dir / 'applications'}}
    sed -e 's/^Name=.*/Name=DropTerm (Dev)/' -e 's/^Exec=.*/Exec={{name}}-dev/' -e 's/^StartupWMClass=.*/StartupWMClass={{appid}}.Dev/' {{desktop-src}} > {{dev-desktop}}

# Kill the running dev instance and restart on the dev image. An
# installed release keeps running — the two identities coexist, each
# answering its own launcher.
dev-reload: (build-debug dev-identity-args) dev-image-install
    -pkill -f '{{dev-kill-pattern}}'
    sleep 0.3
    {{dev-image}}

# Build, install the launcher, and restart on the latest code in one step.
dev: dev-install dev-reload

# Rebuild and restart the overlay on every save under src/ (needs cargo-watch).
dev-watch:
    cargo watch -q -w src -w Cargo.toml -s 'just dev-reload'

# The dev applet's desktop file: the res/ original with the dev name
# and Exec, generated here so the panel contract has one source of
# truth. The stem is the dev identity's app id — the panel matches
# applets by stem, and the release stem must never collide with it.
dev-applet-desktop := dev-data-dir / 'applications' / (applet-appid + '.Dev.desktop')

# Build the applet (debug, dev identity) and install it, its dev
# desktop file, and its icon under ~/.local — a second panel entry,
# "DropTerm (Dev)", that coexists with an installed applet and tracks
# the dev terminal only. Depends on dev-install because the dev
# applet summons the dev launcher. Add it via COSMIC Settings →
# Panel.
dev-applet: dev-install
    cargo build -p {{applet}} {{dev-identity-args}}
    install -Dm0755 {{cargo-target-dir / 'debug' / applet}} {{dev-bin-dir / (applet + '-dev')}}
    install -d {{dev-data-dir / 'applications'}}
    sed -e 's/^Name=.*/Name=DropTerm (Dev)/' -e 's/^Exec=.*/Exec={{applet}}-dev/' {{applet-desktop-src}} > {{dev-applet-desktop}}
    install -Dm0644 {{applet-icon-src}} {{dev-data-dir / 'icons' / 'hicolor' / 'scalable' / 'apps' / applet-icon}}

# Removes the dev footprint — the launcher, the dev terminal's
# desktop file, the dev applet's binary, desktop file, and icon. The
# dev identity's config and session files
# (~/.config/io.github.l1br3_io.CosmicExtDropterm.Dev.ron,
# ~/.local/state/io.github.l1br3_io.CosmicExtDropterm.Dev/) stay
# behind for the user to keep or delete.
dev-uninstall:
    rm -f {{dev-bin}} {{dev-desktop}} {{dev-bin-dir / (applet + '-dev')}} {{dev-applet-desktop}} {{dev-data-dir / 'icons' / 'hicolor' / 'scalable' / 'apps' / applet-icon}}

clean:
    cargo clean

# Stops any release instance still running a binary this justfile
# replaced or removed. A running process keeps its old inode alive
# after `install` overwrites the file — /proc/<pid>/exe then reads
# "<path> (deleted)", and the instance keeps serving the pre-install
# code until manually restarted (the stale-instance footgun). The
# exact-string exe check can never match the dev instance (its own
# image path) or the applet (its own name); only genuinely replaced
# release processes stop. The next summon — keybind, panel applet,
# or `just act toggle` — starts the fresh binary. Running this
# standalone (`just stop-stale-release`) is the one-command restart
# after a manual binary swap. The panel applet refreshes on its own
# schedule and is deliberately left alone.
stop-stale-release:
    #!/usr/bin/env sh
    for pid in $(pgrep -f '{{bin-dst}}' || true); do
        if [ "$(readlink /proc/"$pid"/exe 2>/dev/null)" = '{{bin-dst}} (deleted)' ]; then
            kill "$pid"
            echo "stopped stale release instance (pid $pid); the next summon starts the fresh binary"
        fi
    done

# Installs files (build both artifacts first: just build-all). Checks
# the build prerequisites first (see deps) — a no-op once they exist.
# Ends by stopping any instance still on the replaced binary, so what
# runs after an install is always the installed code.
install: deps
    install -Dm0755 {{bin-src}} {{bin-dst}}
    install -Dm0644 {{desktop-src}} {{desktop-dst}}
    install -Dm0644 {{metainfo-src}} {{metainfo-dst}}
    install -Dm0644 {{icon-src}} {{icon-dst}}
    install -Dm0755 {{applet-bin-src}} {{applet-bin-dst}}
    install -Dm0644 {{applet-desktop-src}} {{applet-desktop-dst}}
    install -Dm0644 {{applet-metainfo-src}} {{applet-metainfo-dst}}
    install -Dm0644 {{applet-icon-src}} {{applet-icon-dst}}
    @{{just_executable()}} stop-stale-release

# Uninstalls installed files (and stops any instance left running
# the removed binary)
uninstall:
    rm -f {{bin-dst}} {{desktop-dst}} {{metainfo-dst}} {{icon-dst}}
    rm -f {{applet-bin-dst}} {{applet-desktop-dst}} {{applet-icon-dst}} {{applet-metainfo-dst}}
    @{{just_executable()}} stop-stale-release

# Regenerate packaging/flatpak/cargo-sources.json from Cargo.lock with
# flatpak-builder-tools' cargo generator (the pop-os/cosmic-flatpak
# convention; their tooling regenerates it at PR review, so it stays
# gitignored here). On a bare Ubuntu python — no pip, no venv package —
# pip bootstraps into the user site via get-pip, and tomlkit (the
# generator's only dependency) installs beside it. Nothing is vendored
# into the repo or installed system-wide.
applet-flatpak-sources:
    curl -fsSL https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py \
        -o /tmp/flatpak-cargo-generator.py
    python3 -m pip --version >/dev/null 2>&1 || { curl -fsSL https://bootstrap.pypa.io/get-pip.py -o /tmp/get-pip.py; python3 /tmp/get-pip.py --user --break-system-packages -q; }
    python3 -c 'import tomlkit' 2>/dev/null || python3 -m pip install --user --break-system-packages -q tomlkit
    python3 /tmp/flatpak-cargo-generator.py \
        Cargo.lock -o {{flatpak-dir / 'cargo-sources.json'}}

# Build the applet flatpak, mirroring cosmic-flatpak's CI invocation
# (deps pulled from flathub into the user installation). State lands
# in .flatpak-builder/, the build tree under packaging/flatpak/build-dir.
applet-flatpak-build:
    flatpak-builder --user --install-deps-from=flathub --force-clean \
        {{flatpak-dir / 'build-dir'}} {{applet-manifest}}

# Install the built applet into the user installation (from the
# builder's cache, so a prior applet-flatpak-build makes this quick).
applet-flatpak-install:
    flatpak-builder --user --install-deps-from=flathub --force-clean --install \
        {{flatpak-dir / 'build-dir'}} {{applet-manifest}}

# Uninstall the applet flatpak from the user installation.
applet-flatpak-remove:
    flatpak uninstall --user -y {{applet-appid}}

# ── Release ──
#
# The release is two commands and a push, because one part of it is
# necessarily post-publish: the applet's flatpak manifest pins the
# *published* source archive (URL + sha256), and neither exists until
# release.yml has run. The whole runbook lives at
# docs/development.md#releasing.
#
# `just release X.Y.Z` cuts the release: it verifies the curated
# release edits (the releases.md section, both metainfo entries, the
# re-pointed install URLs — all written first, committed or left in
# the tree), bumps both crates' versions, refreshes Cargo.lock (the
# --locked recipes would otherwise fail), runs the full gate (`just
# check` plus the appstream validation CI enforces), commits
# everything as one release-hygiene commit (the v0.2.0 shape), and
# tags. It never pushes: the push IS the publish (release.yml fires
# on the tag), so it stays the maintainer's explicit act.
#
# The version may be given with or without the leading `v`. A failed
# gate leaves the version bumps uncommitted in the tree — fix, re-run.
release version:
    #!/usr/bin/env sh
    set -eu

    ver='{{version}}'
    ver="${ver#v}"
    echo "$ver" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' || {
        echo "release: version must be MAJOR.MINOR.PATCH, got '$ver'" >&2
        exit 1
    }

    branch="$(git rev-parse --abbrev-ref HEAD)"
    [ "$branch" = "main" ] || {
        echo "release: cut releases from main, currently on '$branch'" >&2
        exit 1
    }
    git rev-parse -q --verify "refs/tags/v$ver" >/dev/null && {
        echo "release: tag v$ver already exists" >&2
        exit 1
    }

    # Only release files may be in flight — anything else would ride
    # into the release commit silently. Untracked files are fine;
    # they are never added.
    bad=$(git diff --name-only HEAD | grep -v -E '^(Cargo\.toml|applet/Cargo\.toml|Cargo\.lock|README\.md|ROADMAP\.md|docs/releases\.md|docs/testing\.md|res/[^/]+\.metainfo\.xml)$' || true)
    [ -z "$bad" ] || {
        echo "release: unrelated changes in the tree — commit or stash them first:" >&2
        echo "$bad" | sed 's/^/  /' >&2
        exit 1
    }

    old="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n1)"
    [ "$ver" != "$old" ] || {
        echo "release: Cargo.toml already reads $ver" >&2
        exit 1
    }
    today="$(date +%Y-%m-%d)"

    # The curated pieces must already exist, in today's dated shape —
    # fail before touching anything so a half-release is never left
    # behind by missing notes.
    for f in {{metainfo-src}} {{applet-metainfo-src}}; do
        grep -q "<release version=\"$ver\" date=\"$today\">" "$f" || {
            echo "release: $f carries no <release version=\"$ver\" date=\"$today\"> entry — write it first (docs/development.md#releasing)" >&2
            exit 1
        }
    done
    grep -q "^## v$ver ($today)$" docs/releases.md || {
        echo "release: docs/releases.md carries no '## v$ver ($today)' section — write it first" >&2
        exit 1
    }
    # Every living download URL must already point at the new version.
    # releases.md keeps its history sections, and packaging/flatpak
    # deliberately pins the last PUBLISHED release until
    # release-flatpak-pin runs — both are excluded.
    stale=$(grep -rn --exclude-dir=_site 'releases/download/v[0-9]' README.md docs res 2>/dev/null \
        | grep -v "releases/download/v$ver/" | grep -v '^docs/releases\.md:' || true)
    [ -z "$stale" ] || {
        echo "release: stale release URLs still point elsewhere than v$ver:" >&2
        echo "$stale" | sed 's/^/  /' >&2
        exit 1
    }

    # Both crates bump together (first `version =` line only — the
    # manifests carry dependency versions too), and the lock follows
    # so every --locked recipe keeps holding.
    sed -i "0,/^version = \"$old\"$/s//version = \"$ver\"/" Cargo.toml
    sed -i "0,/^version = \"$old\"$/s//version = \"$ver\"/" applet/Cargo.toml
    [ "$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n1)" = "$ver" ] || {
        echo "release: Cargo.toml's package version no longer reads $old — bump it by hand" >&2
        exit 1
    }
    [ "$(sed -n 's/^version = "\(.*\)"$/\1/p' applet/Cargo.toml | head -n1)" = "$ver" ] || {
        echo "release: applet/Cargo.toml's package version no longer reads $old — bump it by hand" >&2
        exit 1
    }
    cargo update -w
    cargo metadata --locked --format-version 1 >/dev/null

    # The gate: everything CI checks on a PR, plus the appstream
    # validation (skipped silently where appstreamcli is absent — CI
    # is the backstop).
    {{just_executable()}} check
    if command -v appstreamcli >/dev/null 2>&1; then
        appstreamcli validate --no-net {{metainfo-src}} {{applet-metainfo-src}}
    fi

    git add Cargo.toml applet/Cargo.toml Cargo.lock README.md ROADMAP.md \
        docs/releases.md docs/testing.md {{metainfo-src}} {{applet-metainfo-src}}
    git commit -m "chore: release hygiene for v$ver — versions, release notes, install URLs"
    git tag -a "v$ver" -m "Release $ver"

    echo "v$ver committed and tagged. To publish:"
    echo "  git push origin main && git push origin v$ver"
    echo "release.yml then builds both arches + vendor.tar, guards tag == Cargo.toml, and publishes the GitHub release."
    echo "Once the release page exists: just release-flatpak-pin $ver"

# Re-pins the applet flatpak manifest to an already-PUBLISHED release:
# downloads the release's source archive, records its sha256, and
# commits the manifest's URL + hash pair (the one release edit that
# cannot ride the release commit — neither the URL nor the hash
# exists before publish). Runs after `just release` and the push.
release-flatpak-pin version:
    #!/usr/bin/env sh
    set -eu
    ver='{{version}}'
    ver="${ver#v}"
    url="https://github.com/l1br3-io/cosmic-ext-dropterm/releases/download/v$ver/v$ver.tar.gz"
    tmp="$(mktemp)"
    trap 'rm -f "$tmp"' EXIT
    curl -fsSL -o "$tmp" "$url" || {
        echo "release-flatpak-pin: $url is not downloadable yet — publish the release first" >&2
        exit 1
    }
    sha="$(sha256sum "$tmp" | cut -d' ' -f1)"
    sed -i "s|releases/download/v[0-9.]*/v[0-9.]*\.tar\.gz|releases/download/v$ver/v$ver.tar.gz|" {{applet-manifest}}
    sed -i "s|\"sha256\": \"[0-9a-f]\{64\}\"|\"sha256\": \"$sha\"|" {{applet-manifest}}
    [ "$(grep -c "releases/download/v$ver/v$ver\.tar\.gz" {{applet-manifest}})" = "1" ] || {
        echo "release-flatpak-pin: manifest's archive URL did not re-pin exactly once — pin by hand" >&2
        exit 1
    }
    [ "$(grep -c "\"sha256\": \"$sha\"" {{applet-manifest}})" = "1" ] || {
        echo "release-flatpak-pin: manifest's sha256 did not re-pin exactly once — pin by hand" >&2
        exit 1
    }
    git add {{applet-manifest}}
    git commit -m "chore: the applet manifest pins the v$ver archive"
    echo "pinned and committed. To publish: git push origin main"
