# Recipes are silent on success and announce only on failure. `set quiet`
# stops just from echoing each recipe line; a failing recipe still prints
# just's `error: Recipe ... failed` line plus the underlying tool's stderr.
set quiet

name := 'cosmic-ext-dropterm'
appid := 'io.github.l1br3_io.CosmicExtDropterm'

rootdir := ''
prefix := '/usr'

base-dir := absolute_path(clean(rootdir / prefix))

cargo-target-dir := env('CARGO_TARGET_DIR', 'target')
bin-src := cargo-target-dir / 'release' / name
bin-dst := base-dir / 'bin' / name
debug-bin := cargo-target-dir / 'debug' / name

# Where the dev launcher lives. ~/.local/bin comes before /usr/bin on the
# COSMIC session PATH, so it shadows an installed copy while developing.
dev-bin-dir := env('XDG_BIN_HOME', home_directory() / '.local' / 'bin')
dev-bin := dev-bin-dir / name

# `pkill -f` pattern with a bracketed first character, so it cannot match this
# recipe's own `sh -c` command line. Update the name here if the binary is
# renamed.
dev-kill-pattern := '[c]osmic-ext-dropterm'

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

# `cargo fmt --check` + `cargo clippy -D warnings` + `cargo test` +
# the docs link gate, all across the workspace. Union features are
# fine here: nothing renders in tests or lints. Release/install
# recipes stay per-package.
check: fmt-check clippy test docs-check

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --check --all

clippy *args:
    cargo clippy --all-targets --workspace --locked {{args}} -- -D warnings

test *args:
    cargo test --workspace --locked {{args}}

# Docs drift gate: every internal markdown link — relative path and
# in-file anchor — in README.md, ROADMAP.md, and docs/ must resolve.
# External (http/mailto) links are out of scope. Silent on success;
# failures name the file and the dangling link.
docs-check:
    #!/usr/bin/env python3
    import re
    from pathlib import Path
    files = [Path("README.md"), Path("ROADMAP.md"), *sorted(Path("docs").rglob("*.md"))]
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

# Send a CLI action to the running instance, like a COSMIC keybind would:
# `just act toggle`, `just act new-tab --cwd /tmp`. Runs the binary from
# PATH (`just dev-install` shadows an installed copy with the debug build);
# the running instance parses the action itself, so any build drives it.
act *args:
    {{name}} {{args}}

# Toggle the overlay (the default action of a bare launch).
toggle:
    {{name}} toggle

# Show the overlay without toggling.
show:
    {{name}} show

# Hide the overlay without toggling.
hide:
    {{name}} hide

# Open a new tab in the running instance, e.g.
# `just new-tab --cwd /tmp` or `just new-tab --cmd htop`.
new-tab *args:
    {{name}} new-tab {{args}}

# Compiles with the debug profile. Per-package on purpose — a plain
# `cargo build` now spans the workspace and unifies features across
# both crates (see build-applet for why that must not produce
# release artifacts).
build-debug *args:
    cargo build -p {{name}} {{args}}

# Compiles with the release profile
build-release *args: (build-debug '--release' args)

# Compiles the applet with the release profile. Per-package on
# purpose: a unified workspace release build would hand the applet a
# wgpu-enabled libcosmic through feature unification, and the wgpu
# renderer falls back to llvmpipe on panel surfaces.
build-applet *args:
    cargo build -p {{applet}} --release {{args}}

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

# Symlink the debug build into ~/.local/bin so a COSMIC shortcut can run it.
dev-install: build-debug
    install -d {{dev-bin-dir}}
    ln -sfn {{justfile_directory() / debug-bin}} {{dev-bin}}

# Kill the running instance and restart on the newly built binary.
dev-reload: build-debug
    -pkill -f '{{dev-kill-pattern}}'
    sleep 0.3
    {{debug-bin}}

# Build, install the launcher, and restart on the latest code in one step.
dev: dev-install dev-reload

# Rebuild and restart the overlay on every save under src/ (needs cargo-watch).
dev-watch:
    cargo watch -q -w src -w Cargo.toml -s 'just dev-reload'

# Build the applet (debug) and install it, its desktop file, and its
# icon under ~/.local so it can be added via COSMIC Settings → Panel.
dev-applet:
    cargo build -p {{applet}}
    install -Dm0755 {{cargo-target-dir / 'debug' / applet}} {{dev-bin-dir / applet}}
    install -Dm0644 {{applet-desktop-src}} {{dev-data-dir / 'applications' / applet-desktop}}
    install -Dm0644 {{applet-icon-src}} {{dev-data-dir / 'icons' / 'hicolor' / 'scalable' / 'apps' / applet-icon}}

clean:
    cargo clean

# Installs files (build both artifacts first: just build-all)
install:
    install -Dm0755 {{bin-src}} {{bin-dst}}
    install -Dm0644 {{desktop-src}} {{desktop-dst}}
    install -Dm0644 {{metainfo-src}} {{metainfo-dst}}
    install -Dm0644 {{icon-src}} {{icon-dst}}
    install -Dm0755 {{applet-bin-src}} {{applet-bin-dst}}
    install -Dm0644 {{applet-desktop-src}} {{applet-desktop-dst}}
    install -Dm0644 {{applet-icon-src}} {{applet-icon-dst}}

# Uninstalls installed files
uninstall:
    rm -f {{bin-dst}} {{desktop-dst}} {{metainfo-dst}} {{icon-dst}}
    rm -f {{applet-bin-dst}} {{applet-desktop-dst}} {{applet-icon-dst}}

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
