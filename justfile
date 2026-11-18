# Recipes are silent on success and announce only on failure. `set quiet`
# stops just from echoing each recipe line; a failing recipe still prints
# just's `error: Recipe ... failed` line plus the underlying tool's stderr.
set quiet

# Run a command via the shell, printing its combined output only when it
# fails. Agent-facing gates route through this so a green run costs zero
# output tokens while a red one replays the full log with its exit code.
[private]
quietly command:
    #!/usr/bin/env bash
    set -euo pipefail
    # V=1 passes the command straight through (the make convention).
    if [ "${V:-0}" != "0" ]; then
        exec bash -c '{{command}}'
    fi
    out="$(mktemp)"
    set +e
    bash -c '{{command}}' >"$out" 2>&1
    status=$?
    set -e
    if [ "$status" -ne 0 ]; then
        cat "$out"
        rm -f "$out"
        exit "$status"
    fi
    rm -f "$out"

name := 'dropterm'
appid := 'io.github.l1br3_io.dropterm'

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

# Where the dev launcher lives. The launcher is a fence-wrapping
# script (see `dev-install`), not a bare symlink, so every launch of
# the dev identity rides the memory cap. It carries a `-dev` suffix
# so it never shadows an installed release: `dropterm` on
# PATH is always the installed copy, `dropterm-dev` always
# the fenced dev build.
dev-bin-dir := env('XDG_BIN_HOME', home_directory() / '.local' / 'bin')
dev-bin := dev-bin-dir / (name + '-dev')

# The dev identity's own binary image. The feature-flagged build is
# COPIED here (never symlinked to the shared debug bin): a plain
# `cargo build`/`cargo test` — including inside `just check` —
# rebuilds `target/debug/dropterm` WITHOUT the
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

# The dev instance's memory fence — `just mem-churn`'s daily twin:
# every `dev-reload` launch rides a systemd-run scope, so a runaway
# dev build dies at its own cgroup (the telemetry curve waits in the
# `.Dev` state log for the post-mortem) instead of OOM-ing the host
# the 47.1 GB way. Roomier than the probe's pin on purpose: the
# scope inherits every PTY child, and the dev instance's daily job
# is running this repo's builds inside itself. Both overridable by
# env for a heavy session; the churn probe's cap stays hard-pinned.
dev-mem-max := env('DROPTERM_DEV_MEM_MAX', '4G')
dev-mem-high := env('DROPTERM_DEV_MEM_HIGH', '3G')

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
# target path with a `( |$)` guard so it stops at the binary name.
# Each alternative's first character is bracketed so the pattern
# cannot match this recipe's own `sh -c` command line. The installed
# release never matches — leaving it running through a dev cycle is
# the point of the dev-identity split.
dev-kill-pattern := '[d]ropterm-dev|[t]arget/debug/dropterm-dev-image( |$)|[t]arget/debug/dropterm( |$)'

desktop := appid + '.desktop'
desktop-src := 'res' / desktop
desktop-dst := clean(rootdir / prefix) / 'share' / 'applications' / desktop

metainfo := appid + '.metainfo.xml'
metainfo-src := 'res' / metainfo
metainfo-dst := clean(rootdir / prefix) / 'share' / 'metainfo' / metainfo

icon-src := 'res' / 'icons' / 'hicolor' / 'scalable' / 'apps' / (appid + '.svg')
icon-dst := clean(rootdir / prefix) / 'share' / 'icons' / 'hicolor' / 'scalable' / 'apps' / (appid + '.svg')

# The installed extended completions tier (suggest-embed spec): the
# generated data file the XDG walk finds behind the
# `autosuggestions_completions_extended` toggle — never embedded,
# sitting in the app id's own share directory.
completions-extended-src := 'data' / 'completions-extended.toml'
completions-extended-dir := clean(rootdir / prefix) / 'share' / appid
completions-extended-dst := completions-extended-dir / 'completions-extended.toml'

# Where the dev identity's desktop file lands.
dev-data-dir := env('XDG_DATA_HOME', home_directory() / '.local' / 'share')

# The dev terminal's desktop file: the res/ original with the dev
# name, Exec, and StartupWMClass, generated here so the desktop
# contract has one source of truth. The stem
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
    {{just_executable()}} quietly 'cargo clippy --all-targets --workspace --locked {{args}} -- -D warnings'
    {{just_executable()}} quietly 'cargo clippy --all-targets -p {{name}} --locked --features dev-identity {{args}} -- -D warnings'

test *args:
    {{just_executable()}} quietly 'cargo test --workspace --locked {{args}}'
    {{just_executable()}} quietly 'cargo test -p {{name}} --locked --features dev-identity {{args}}'

# The incident probe as a recipe (ram-leak rl-1): the real-rc
# toggle churn test is ignore-gated because its growth path must
# only ever run under a hard cap — until now the pinned
# systemd-run scope lived only in the test's doc comment, retyped
# by hand each run. The caps are the incident's own (2G max,
# 1536M high, no swap): a live regression dies at the cgroup with
# the per-cycle VmRSS curve already printed, never at the host's
# OOM killer. Not routed through `quietly` — the curve is the
# deliverable. Owner-run: the input is the real login rc, which
# no agent shell has.
mem-churn:
    systemd-run --user --scope -p MemoryMax=2G -p MemoryHigh=1536M \
      -p MemorySwapMax=0 -- cargo test -p {{name}} --locked --lib \
      real_rc_toggle_churn_rss_curve -- --ignored --test-threads=1 \
      --nocapture

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
    echo "serving at http://127.0.0.1:$port/dropterm/"
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
    #!/usr/bin/env bash
    # pipefail: the rustup bootstrap is a `curl | sh` pipeline — without
    # it, a dead curl hands sh an EOF, sh exits 0, and deps reports
    # success with cargo never installed.
    set -euo pipefail

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
            # Not a package-manager gap on non-Linux: DropTerm targets
            # the COSMIC desktop, so name the real reason instead of a
            # package guessing game.
            case "$(uname -s)" in
                Darwin)
                    echo "deps: DropTerm targets the COSMIC desktop — build on Linux; macOS has neither the compositor nor its libraries" >&2
                    ;;
                *)
                    echo "deps: no supported package manager (apt-get, dnf, pacman, zypper)" >&2
                    echo "deps: install the equivalents of:$missing and re-run" >&2
                    ;;
            esac
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

# Compiles with the debug profile.
build-debug *args: deps
    {{just_executable()}} quietly 'cargo build -p {{name}} {{args}}'

# Compiles with the release profile, throttled (see build-jobs).
build-release *args: deps
    {{just_executable()}} quietly 'nice -n {{build-nice}} cargo build -p {{name}} --release -j {{build-jobs}} {{args}}'

# The release artifact, in the shape `install` expects. Exists as one
# recipe so callers get the artifact without `build-release`'s own
# argument plumbing in the way.
build-all: build-release

# Vendored offline builds (the System76 release convention): `vendor`
# snapshots the whole dependency graph — including the git-pinned
# vte/alacritty forks — into vendor.tar together with the source-replacement
# config cargo vendor prints (a git dependency needs its own
# [source."url"] redirect, so the config cannot be hand-written). The
# working tree is cleaned afterward so dev builds keep using git deps.
vendor:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p .cargo
    # pipefail: a failed `cargo vendor` must not hide behind `head`'s
    # exit 0 and ship an incomplete vendor.tar as success. Its stderr
    # (the per-crate "Vendoring …" progress) is captured and replayed
    # only on failure.
    log="$(mktemp)"
    if cargo vendor --locked 2>"$log" | head -n -1 > .cargo/config.toml; then
        rm -f "$log"
    else
        status=$?
        cat "$log"
        rm -f "$log"
        exit "$status"
    fi
    echo 'directory = "vendor"' >> .cargo/config.toml
    tar -cf vendor.tar vendor .cargo/config.toml
    rm -rf vendor .cargo

# Extracts vendor/ and .cargo/config.toml from vendor.tar (see vendor).
vendor-extract:
    rm -rf vendor .cargo
    tar -xf vendor.tar

# Compiles the release artifact from vendor.tar, --frozen --offline:
# the proof that vendor.tar is a complete dependency snapshot.
build-vendored *args: vendor-extract (build-release '--frozen' '--offline' args)

# The raw debug-bin path (`cargo run`, release identity) — fenced
# like every other local launch of the app. cargo's build rides
# inside the scope too; a fence-killed build here means the cap was
# genuinely earning its keep.
run *args:
    systemd-run --user --scope --quiet -p MemoryMax={{dev-mem-max}} \
      -p MemoryHigh={{dev-mem-high}} -p MemorySwapMax=0 -- \
      cargo run {{args}}

# Installs the dev launcher at `~/.local/bin/dropterm-dev` — a
# fence-wrapping script, not a bare symlink, so EVERY launch of the
# dev identity (a COSMIC shortcut, the act recipes' cold starts, a
# manual run) rides the capped scope, not just the `just dev*`
# recipes. The `-dev` name leaves an installed release's plain name
# untouched. Also installs the dev identity's desktop file (see
# `dev-desktop`).
dev-install: (build-debug dev-identity-args) dev-image-install
    #!/usr/bin/env sh
    set -eu
    install -d {{dev-bin-dir}}
    # rm FIRST: an older launcher may be a symlink to the image, and
    # `cat >` through it would overwrite the BINARY with this script
    # (the 2026-11-18 recursion — every launch then exec'd the script
    # on itself). Removing before writing guarantees a regular file.
    rm -f {{dev-bin}}
    # The fence's defaults are baked here as literals (launch-time
    # env override in the script); the quoted heredoc keeps ${…} for
    # the launcher itself while just bakes the image path in.
    cat > {{dev-bin}} <<'EOF'
    #!/bin/sh
    exec systemd-run --user --scope --quiet \
      -p MemoryMax="${DROPTERM_DEV_MEM_MAX:-4G}" \
      -p MemoryHigh="${DROPTERM_DEV_MEM_HIGH:-3G}" \
      -p MemorySwapMax=0 -- {{dev-image}} "$@"
    EOF
    chmod 0755 {{dev-bin}}
    install -d {{dev-data-dir / 'applications'}}
    sed -e 's/^Name=.*/Name=DropTerm (Dev)/' -e 's/^Exec=.*/Exec={{name}}-dev/' -e 's/^StartupWMClass=.*/StartupWMClass={{appid}}.Dev/' {{desktop-src}} > {{dev-desktop}}
    # The installed extended tier for the dev identity: both seats,
    # so the .Dev walk's primary and its un-suffixed fallback both
    # find what a release install stages.
    install -Dm0644 {{completions-extended-src}} {{dev-data-dir / appid / 'completions-extended.toml'}}
    install -Dm0644 {{completions-extended-src}} {{dev-data-dir / (appid + '.Dev') / 'completions-extended.toml'}}

# Kill the running dev instance and restart on the dev image, under
# the memory fence: the systemd-run scope holds the instance and
# every PTY child it spawns, so a runaway dies at the cgroup, host
# untouched. An installed release keeps running — the two
# identities coexist, each answering its own launcher.
dev-reload: (build-debug dev-identity-args) dev-image-install
    -pkill -f '{{dev-kill-pattern}}'
    sleep 0.3
    systemd-run --user --scope --quiet -p MemoryMax={{dev-mem-max}} \
      -p MemoryHigh={{dev-mem-high}} -p MemorySwapMax=0 -- {{dev-image}}

# Build, install the launcher, and restart on the latest code in one step.
dev: dev-install dev-reload

# Rebuild and restart the overlay on every save under src/ (needs cargo-watch).
dev-watch:
    cargo watch -q -w src -w Cargo.toml -s 'just dev-reload'

# Removes the dev footprint — the launcher and the dev terminal's
# desktop file. The dev identity's config and session files
# (~/.config/io.github.l1br3_io.dropterm.Dev.ron,
# ~/.local/state/io.github.l1br3_io.dropterm.Dev/) stay
# behind for the user to keep or delete.
dev-uninstall:
    rm -f {{dev-bin}} {{dev-desktop}}
    rm -rf {{dev-data-dir / appid}} {{dev-data-dir / (appid + '.Dev')}}

clean:
    cargo clean

# Stops any release instance still running a binary this justfile
# replaced or removed. A running process keeps its old inode alive
# after `install` overwrites the file — /proc/<pid>/exe then reads
# "<path> (deleted)", and the instance keeps serving the pre-install
# code until manually restarted (the stale-instance footgun). The
# exact-string exe check can never match the dev instance (its own
# image path); only genuinely replaced release processes stop. The
# next summon — keybind or `just act toggle` — starts the fresh
# binary. Running this standalone (`just stop-stale-release`) is the
# one-command restart after a manual binary swap.
stop-stale-release:
    #!/usr/bin/env sh
    # -e: a failed kill must abort before the success echo below — never
    # announce a stop that did not happen.
    set -eu
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
    install -Dm0644 {{completions-extended-src}} {{completions-extended-dst}}
    @{{just_executable()}} stop-stale-release

# Uninstalls installed files (and stops any instance left running
# the removed binary)
uninstall:
    rm -f {{bin-dst}} {{desktop-dst}} {{metainfo-dst}} {{icon-dst}}
    rm -rf {{completions-extended-dir}}
    @{{just_executable()}} stop-stale-release

# ── COSMIC validation VM ──
#
# Host-side plumbing for walking the manual checklist's runtime rows
# inside a clean Pop!_OS COSMIC VM (the item-5 evidence route; the
# full recipe lives at docs/development.md#the-cosmic-validation-vm).
# Delegates to scripts/vm.sh — ported from cosmic-ext-airlock's CP4
# harness — which drives the guest through the QEMU guest agent (no
# SSH): `just vm-status` is the preflight doctor, `just vm-list` names
# the domains and their guest users, `just vm-help` the command
# reference. The default target is the sibling repo's existing
# PopOs24 domain on this host, but every recipe that acts on a VM
# takes the libvirt domain as an optional argument — `just vm-push
# OtherVm`, `just vm-exec 'pgrep -a cosmic-comp' OtherVm` — and the
# recipes that use the guest user take it right after the domain:
# `just vm-status OtherVm alice`. The variable forms still override
# per run: `VM_NAME=OtherVm VM_USER=alice just vm-push`, or `just
# vm-name=OtherVm vm-user=alice vm-push` (assignment before the
# recipe). BRIDGE_IP and PORT stay variables.
vm-name := env_var_or_default("VM_NAME", "PopOs24")
vm-user := env_var_or_default("VM_USER", "jpt")
# vm-push's tarball (vm-install's guest-side source, staged under the
# ignored target/), so neither the tarball nor the staging tree
# (target/vm-stage) ever shows up as untracked files.
vm-tarball := cargo-target-dir / 'dropterm-vm.tar.gz'

vm-help:
    scripts/vm.sh help

# The libvirt domains on this host — the targets the vm-* recipes
# accept (`just vm-push <domain>`) — with each domain's state and
# guest users (the candidates for VM_USER).
vm-list:
    VM_NAME={{vm-name}} VM_USER={{vm-user}} scripts/vm.sh list

vm-status vm=vm-name user=vm-user:
    VM_NAME={{vm}} VM_USER={{user}} scripts/vm.sh status

# Run a shell command in the guest as root, positionally (just's
# `cmd=value` form leaks the `cmd=` prefix into the value for
# space-carrying arguments), optionally naming the target domain:
# just vm-exec 'pgrep -a cosmic-comp' [domain]
vm-exec cmd vm=vm-name:
    VM_NAME={{vm}} scripts/vm.sh exec "{{cmd}}"

# Run it as the desktop user inside the session env (runtime dir, session
# bus, Wayland socket): just vm-exec-user 'dropterm toggle' [domain] [user]
vm-exec-user cmd vm=vm-name user=vm-user:
    VM_NAME={{vm}} VM_USER={{user}} scripts/vm.sh exec-user "{{cmd}}"

# Transfer a file into the guest (sha256 verified both sides)
vm-put file dest vm=vm-name user=vm-user:
    VM_NAME={{vm}} VM_USER={{user}} scripts/vm.sh put "{{file}}" "{{dest}}"

# Build the release artifact and ship the install payload: builds,
# stages the usr/ tree exactly as release CI does (release.yml runs
# `just rootdir=stage install` before tarring), tars it, and transfers
# the tarball into the guest (sha256 verified both sides by vm.sh put),
# leaving it at /root/dropterm-vm.tar.gz. Nothing the guest runs changes
# until `just vm-install`, which can then re-run the install half
# without rebuilding. Install and update are the same install: the
# tarball always overwrites the five files `install` stages.
vm-push vm=vm-name user=vm-user:
    #!/usr/bin/env sh
    set -eu
    {{just_executable()}} build-all
    rm -rf {{cargo-target-dir / 'vm-stage'}} {{vm-tarball}}
    {{just_executable()}} rootdir='{{cargo-target-dir / 'vm-stage'}}' install
    tar -C {{cargo-target-dir / 'vm-stage'}} -czf {{vm-tarball}} usr
    VM_NAME={{vm}} VM_USER={{user}} scripts/vm.sh put {{vm-tarball}} /root/dropterm-vm.tar.gz
    echo "vm: pushed to {{vm}}; run: just vm-install {{vm}}"

# The guest-side half: unpacks the pushed tarball over / (--no-same-owner:
# the tarball records the host uid, and a root-side extract would chown
# /usr files to it), refreshes the desktop database, and stops anything
# still running a replaced binary — the guest-side half of
# stop-stale-release, since a warm process keeps its old inode.
# Installs whatever the last push to that domain left at /root. The
# stop matches an anchored full-command pattern: the `(^|/)…([ ]|$)`
# anchors keep the exec wrapper shells (name mid-command-line) and the
# pattern's own text out of the match. The next summon (keybind, CLI)
# runs this build; no relogin.
vm-install vm=vm-name:
    VM_NAME={{vm}} scripts/vm.sh exec 'test -f /root/dropterm-vm.tar.gz || { echo "vm: no pushed tarball in the guest — run: just vm-push {{vm}}" >&2; exit 1; }; tar -xzf /root/dropterm-vm.tar.gz -C / --no-same-owner && rm -f /root/dropterm-vm.tar.gz && { update-desktop-database /usr/share/applications 2>/dev/null || true; }'
    VM_NAME={{vm}} scripts/vm.sh exec 'pkill -f "(^|/){{name}}([ ]|$)" && echo "vm: stopped a running dropterm; the next summon runs this build" || echo "vm: no dropterm instance was running"'
    echo "vm: installed into {{vm}}; the next summon runs this build"

# Capture the VM's display (evidence screenshots)
vm-screenshot file vm=vm-name:
    VM_NAME={{vm}} scripts/vm.sh screenshot "{{file}}"

vm-snapshot name="checkpoint" vm=vm-name:
    VM_NAME={{vm}} scripts/vm.sh snapshot "{{name}}"

vm-restore name vm=vm-name:
    VM_NAME={{vm}} scripts/vm.sh restore "{{name}}"

# ── Release ──
#
# The release is one command and a push. The whole runbook lives at
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
    bad=$(git diff --name-only HEAD | grep -v -E '^(dropterm/Cargo\.toml|Cargo\.lock|README\.md|ROADMAP\.md|docs/releases\.md|docs/testing\.md|res/[^/]+\.metainfo\.xml)$' || true)
    [ -z "$bad" ] || {
        echo "release: unrelated changes in the tree — commit or stash them first:" >&2
        echo "$bad" | sed 's/^/  /' >&2
        exit 1
    }

    old="$(sed -n 's/^version = "\(.*\)"$/\1/p' dropterm/Cargo.toml | head -n1)"
    [ "$ver" != "$old" ] || {
        echo "release: Cargo.toml already reads $ver" >&2
        exit 1
    }
    today="$(date +%Y-%m-%d)"

    # The curated pieces must already exist, in today's dated shape —
    # fail before touching anything so a half-release is never left
    # behind by missing notes.
    for f in {{metainfo-src}}; do
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
    # releases.md keeps its history sections — excluded.
    stale=$(grep -rn --exclude-dir=_site 'releases/download/v[0-9]' README.md docs res 2>/dev/null \
        | grep -v "releases/download/v$ver/" | grep -v '^docs/releases\.md:' || true)
    [ -z "$stale" ] || {
        echo "release: stale release URLs still point elsewhere than v$ver:" >&2
        echo "$stale" | sed 's/^/  /' >&2
        exit 1
    }

    # The one package bumps (first `version =` line only — the
    # manifest carries dependency versions too), and the lock follows
    # so every --locked recipe keeps holding.
    sed -i "0,/^version = \"$old\"$/s//version = \"$ver\"/" dropterm/Cargo.toml
    [ "$(sed -n 's/^version = "\(.*\)"$/\1/p' dropterm/Cargo.toml | head -n1)" = "$ver" ] || {
        echo "release: dropterm/Cargo.toml's package version no longer reads $old — bump it by hand" >&2
        exit 1
    }
    cargo update -w
    cargo metadata --locked --format-version 1 >/dev/null

    # The gate: everything CI checks on a PR, plus the appstream
    # validation (skipped silently where appstreamcli is absent — CI
    # is the backstop).
    {{just_executable()}} check
    if command -v appstreamcli >/dev/null 2>&1; then
        appstreamcli validate --no-net {{metainfo-src}}
    fi

    git add dropterm/Cargo.toml Cargo.lock README.md ROADMAP.md \
        docs/releases.md docs/testing.md {{metainfo-src}}
    git commit -m "chore: release hygiene for v$ver — versions, release notes, install URLs"
    git tag -a "v$ver" -m "Release $ver"

    echo "v$ver committed and tagged. To publish:"
    echo "  git push origin main && git push origin v$ver"
    echo "release.yml then builds both arches + vendor.tar, guards tag == Cargo.toml, and publishes the GitHub release."
