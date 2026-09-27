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

# Default recipe: build in release mode
default: build-release

# `cargo fmt --check` + `cargo clippy -D warnings` + `cargo test`
check: fmt-check clippy test

fmt:
    cargo fmt

fmt-check:
    cargo fmt --check

clippy *args:
    cargo clippy --all-targets {{args}} -- -D warnings

test *args:
    cargo test {{args}}

# Compiles with the debug profile
build-debug *args:
    cargo build {{args}}

# Compiles with the release profile
build-release *args: (build-debug '--release' args)

run *args:
    cargo run {{args}}

# Symlink the debug build into ~/.local/bin so a COSMIC shortcut can run it.
dev-install: build-debug
    install -d {{dev-bin-dir}}
    ln -sfn {{justfile_directory() / debug-bin}} {{dev-bin}}
    @echo "COSMIC shortcut Command: {{name}}"

# Kill the running instance and restart on the newly built binary.
dev-reload: build-debug
    -pkill -f '{{dev-kill-pattern}}'
    @sleep 0.3
    {{debug-bin}}

# Build, install the launcher, and restart on the latest code in one step.
dev: dev-install dev-reload

# Rebuild and restart the overlay on every save under src/ (needs cargo-watch).
dev-watch:
    cargo watch -q -w src -w Cargo.toml -s 'just dev-reload'

clean:
    cargo clean

# Installs files
install:
    install -Dm0755 {{bin-src}} {{bin-dst}}
    install -Dm0644 {{desktop-src}} {{desktop-dst}}
    install -Dm0644 {{icon-src}} {{icon-dst}}

# Uninstalls installed files
uninstall:
    rm -f {{bin-dst}} {{desktop-dst}} {{icon-dst}}
