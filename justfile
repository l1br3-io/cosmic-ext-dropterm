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

# Where dev-applet installs the applet's desktop file and icon.
dev-data-dir := env('XDG_DATA_HOME', home_directory() / '.local' / 'share')

# `cargo fmt --check` + `cargo clippy -D warnings` + `cargo test`, all
# across the workspace. Union features are fine here: nothing renders
# in tests or lints. Release/install recipes stay per-package.
check: fmt-check clippy test

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --check --all

clippy *args:
    cargo clippy --all-targets --workspace {{args}} -- -D warnings

test *args:
    cargo test --workspace {{args}}

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
    install -Dm0644 {{icon-src}} {{icon-dst}}
    install -Dm0755 {{applet-bin-src}} {{applet-bin-dst}}
    install -Dm0644 {{applet-desktop-src}} {{applet-desktop-dst}}
    install -Dm0644 {{applet-icon-src}} {{applet-icon-dst}}

# Uninstalls installed files
uninstall:
    rm -f {{bin-dst}} {{desktop-dst}} {{icon-dst}}
    rm -f {{applet-bin-dst}} {{applet-desktop-dst}} {{applet-icon-dst}}
