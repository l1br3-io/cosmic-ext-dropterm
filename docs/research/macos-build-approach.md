# Is the macos-build approach — target-cfg dependency tables, paired honest arms, and a refusal stub — the established pattern?

Date: 2026-11-21 · Record: immutable — corrections land as new rows,
this text never changes. All external facts pinned to their primary
source at fetch date 2026-11-21. The six mb commits reviewed here are
`6edfff5..bcc3a8c` (branch `port/macos-build` vs `main`); every claim
about them below was re-verified by reading the diff and the named
files, not the module summary.

## TL;DR

**The structure the macos-build module shipped is the established
pattern at every layer we checked — with two factual corrections and
one standing obligation.** Target-conditional dependency tables are the
Cargo Book's own mechanism and are what zed's `gpui_platform` uses
verbatim (D1); paired `#[cfg(target_os)]` arms with honest non-Linux
behavior is exactly what `alacritty_terminal` — the engine we fork —
has shipped for years, and what zed did for years before extracting
platform crates in 2026 (D2); the run-but-refuse stub mirrors the
repo's own layer-shell guard contract and the refusal-with-named-remedy
shape rustup and systemctl use (D3); leaving zbus compiled on mac is
supported upstream and costs nothing that ships (D4); the 1.98 mac CI
pin is forced by the manifest system itself, which cannot express a
per-target MSRV — verified by reproducing both the E0658 failure and
cargo's resolution-time refusal on this machine (D5); and the Metal
Toolchain step matches documented reality, including the runner-image
team's own policy (D6). The two corrections: the mb-1 rationale in the
Cargo.toml comment — that wayland-sys's build script would pkg-config a
library no mac host carries — is **not what happens** in either
feature graph (probes on this machine compiled both wayland deps on
macOS with the current feature set), and the 1.98 std API lives in
`gpui-pre-macos`, not `gpui-pre-apple` as the CI and justfile comments
say. The standing obligation: the plan's `SurfaceBackend` seam must
land *with* `macos-surface`, not after it — every peer surveyed
converted paired cfg arms into a seam at the second backend, and the
arms accrete if the conversion waits.

## Method

Primary sources only: the Cargo Book (doc.rust-lang.org/cargo),
upstream source read directly from GitHub (zed-industries/zed,
alacritty/alacritty, wezterm/wezterm, rust-lang/rustup, dbus2/zbus,
actions/runner-images), the rust-lang/rust 1.98.0 release notes, the
freedesktop systemd man page, Apple's own Xcode component-install
documentation, and the locally installed registry sources
(`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/…`). Where a
claim could be tested on this machine (macOS arm64), it was tested:
`cargo tree` per target, two scratch probes compiling the wayland deps
on macOS, a scratch probe of cargo's `rust-version` enforcement, a
reproduction of the E0658 failure under rustc 1.96.0, and a full
`cargo +1.98.0 test --locked --lib` run on this host (1113 passed,
0 failed, 0 ignored — the same shape the CI macos job pins). Scratch
probes ran under `/tmp/wayland-probe{,2}` and `/tmp/msrv-test`; nothing
in the repo was touched but this record and the README row.

## Findings

### 1. D1 — target-conditional dependency tables (adopted, with a corrected rationale)

- **The mechanism is the documented one.** The Cargo Book's
  "Specifying dependencies" page: "Platform-specific dependencies take
  the same format, but are listed under a target section… Rust-like
  #[cfg] syntax will be used to define these sections", with
  `[target.'cfg(windows)'.dependencies]` as its own example — precisely
  the table mb-1 uses (`dropterm/Cargo.toml:152`,
  `[target.'cfg(target_os = "linux")'.dependencies]`). The same page
  closes the feature-driven alternative's door: "you cannot use
  `[target.'cfg(feature = "fancy-feature")'.dependencies]`… There is
  currently no way to add dependencies based on these configuration
  values" — so a target table cannot be made feature-dependent, and a
  feature cannot carry target scoping for a bin crate without
  default-features gymnastics on every consumer.
- **The workspace's resolver choice makes the tables cheap.** The
  resolver page: the version-2 resolver "will avoid unifying features
  in the following situations: Features for target-specific
  dependencies are not enabled if the target is not currently being
  built." Resolver 3 (this workspace, `Cargo.toml` top: `resolver =
  "3"`) is that plus MSRV-aware version fallback; its only change over
  v2 is the `resolver.incompatible-rust-versions` default. Consequence:
  on the mac target the Linux-only features never unify and the crates
  never build. Verified: `cargo tree --locked --target
  x86_64-apple-darwin` on this host resolves **zero** wayland packages
  (and the whole mac graph is 1417 lines against the Linux graph's
  1673). Cargo.lock stores every target's graph, so moving entries
  between tables cannot touch it — the branch's `git diff main..HEAD`
  on both lockfiles is empty, as mb-1 claimed.
- **zed uses the identical mechanism.** zed's
  `crates/gpui_platform/Cargo.toml` (read from master, 2026-11-21)
  selects its backends with the same tables:
  `[target.'cfg(target_os = "macos")'.dependencies] gpui_macos`,
  `[target.'cfg(all(any(target_os = "linux", target_os = "freebsd"),
  not(target_env = "ohos")))'.dependencies] gpui_linux`, and so on.
  The `gpui-pre-platform` 0.3.12 crate this repo pins is that file's
  published snapshot (local registry: `…/gpui-pre-platform-0.3.12/
  Cargo.toml`, same target tables at lines 100–133). The strongest
  precedent for a *bin crate's* wayland-protocol deps, though, is
  closer to home: this is also how the crate was already riding
  platform-skewed deps inside gpui itself.
- **The strongest alternative — a cargo `wayland` feature — does
  nothing the tables don't, and adds a knob.** For a bin crate with no
  downstream consumers, a feature is a user-selectable switch nobody
  should switch: turning it on under macOS would drag wayland-sys into
  the mac build for zero benefit, and `default-features = false` plus
  per-platform re-enabling is strictly more manifest. The tables are
  the honest declaration: these deps are protocol dependencies, and
  the protocol exists on Linux.
- **The caveat — mb-1's stated reason is not demonstrated, and is in
  fact not what the graphs do.** The comment above the new table says
  both deps "ride wayland-sys, whose build script pkg-configs
  libwayland-client — a library no macOS host carries." The build
  script does have that capability (`…/wayland-sys-0.31.11/build.rs`
  line 1: `use pkg_config::Config;`, and on the `client` feature it
  probes `wayland-client` with `.unwrap()` — a hard build failure on a
  host without libwayland). But the probe is gated: the same build.rs
  *returns first* when the `dlopen` feature is set, and in the resolved
  Linux graph the `client_system` + `dlopen` features both arrive from
  the **ashpd** subtree (ashpd 0.13.13 ← gpui-base ← gpui-component;
  verified with `cargo tree -e features -i wayland-backend:0.3.17`), so
  even on Linux the build script early-returns. And un-gated with
  default features on macOS, nothing enables the probe at all:
  `wl-clipboard-rs` 0.9.5 declares **no** default features (its
  manifest's `[features]` has only `dlopen`/`native_lib`;
  `…/wl-clipboard-rs-0.9.5/Cargo.toml:34–40`), and `wayland-client`
  0.31's system-lib path is opt-in via its `system` feature. Tested:
  two scratch probes (`/tmp/wayland-probe`, `/tmp/wayland-probe2`)
  built `wayland-client` alone and with `wl-clipboard-rs` on this
  macOS arm64 host — both succeeded (6–8 s). So the un-gated mac build
  would most likely have compiled, and the gate's real justification
  is the three things the comment doesn't say: honest scope, a smaller
  mac graph, and a guard — because the moment anything does enable
  `client_system` without `dlopen` on the mac graph (ashpd's wayland
  feature leaking target-gates, a future dep flipping
  `wayland-client/system`), the mac build dies at an opaque
  pkg-config error mid-build-script. The table is right; the reason
  written above it is wrong in the record's judgment.

### 2. D2 — cfg gates with paired honest arms (adopted, with a standing obligation)

- **The engine's own upstream gates this way.** `alacritty_terminal`
  — the crate DropTerm forks (`carried/alacritty-terminal`) — picks its
  tty backend with plain cfg module gates and no per-OS crates:
  `alacritty_terminal/src/tty/mod.rs` (master, 2026-11-21) declares
  `#[cfg(not(windows))] mod unix; pub use self::unix::*;` and
  `#[cfg(windows)] pub mod windows; pub use self::windows::*;`. The
  seam is two traits (`EventedReadWrite`, `EventedPty`) that the
  platform modules implement; `tty::new` hands back whichever module's
  type. This is mb-2's shape at the crate scale we actually sit at:
  paired arms, honest None/false behavior per platform, no abstraction
  beyond what the difference demands.
- **zed ordered it exactly as the plan record does: cfg first, trait
  seam with the second backend, per-OS crates only at scale.** gpui
  lived for years as one crate with cfg-selected platform modules —
  `crates/gpui/src/platform.rs` (master, 2026-11-21) still carries
  those gates on methods (`#[cfg(target_os = "macos")] fn
  set_traffic_light_position…`, `#[cfg(target_os = "windows")] fn
  get_raw_handle…`, `#[cfg(all(target_os = "linux", feature =
  "wayland"))] pub mod layer_shell;`) even after the split — and only
  on 2026-02-19 did zed extract the per-OS crates (commit on
  `crates/gpui_platform/Cargo.toml`: "gpui: Extract gpui_platform out
  of gpui (#49277)", 2026-02-19). The `Platform` trait was the seam
  before the crates were; the crates arrived when macOS + Linux +
  Windows + web + iOS backends justified the boundary. DropTerm today
  has one backend and a second named — the crate split's trigger has
  not fired here.
- **wezterm's third shape confirms the ladder.** `pty/src/lib.rs`
  (master, 2026-11-21): `#[cfg(unix)] pub mod unix; #[cfg(windows)]
  pub mod win;` with the `PtySystem` trait and a
  `native_pty_system()` factory returning `Box<dyn PtySystem>` —
  cfg-picked modules behind one trait, one crate, no platform crates.
- **The repo's own prior art is the engine's `/proc` arms.** The
  engine already carried `#[cfg(target_os = "linux")]` arms reading
  `/proc` with non-Linux `None`/`false` fallbacks
  (`dropterm/src/core/terminal/engine.rs:370,403,426,449,476` —
  pre-existing, not added by this branch); mb-2 extended the same
  grammar to the frontend (`main.rs:21` crate-level
  `cfg_attr(not(target_os = "linux"), allow(dead_code))`,
  `startup.rs:63–87` paired `startup_verdict` arms,
  `terminal/clipboard.rs` five paired functions,
  `terminal/view.rs` the `shell` field + `adopt_shell` + split
  `dispatch_shell_verb`, `engine.rs:4423` the one `ssh_destination`
  attr). The honest-arm discipline — no-op writes, `None` reads, a
  logged no-shell verb — matches how the engine arms behave and keeps
  the portable view tree compiled so the parity modules inherit it.
- **The strongest alternative — introduce a trait seam
  (`SurfaceBackend`) now — is premature by the peers' own ordering.**
  A trait over exactly one implementation is a seam with no joint; the
  arms currently have one honest shape each (no shell, no clipboard
  seat, no backend), and zed/alacritty/wezterm all paid for the
  abstraction only when the second backend existed to conflict with
  the first. The plan record already rules `SurfaceBackend` "one impl
  per protocol" for the later phases — the correct reading of this
  survey is that the seam's landing date is *macos-surface's first
  commit*, not a later cleanup: alacritty's traits arrived with the
  windows split, and every extra `#[cfg]` arm shipped before a seam
  becomes an arm the seam has to migrate. Adopted-with-caveat for that
  reason: the structure is established; the timing rule it implies is
  binding.

### 3. D3 — the honest-refusal stub (adopted)

- **The repo's own contract is the binding precedent, and the stub
  mirrors it.** Core's layer-shell refusal has shipped the exact shape
  since the libcosmic frontend:
  `dropterm/src/core/transport.rs:199` `unsupported_startup_error`
  builds "this session offers no zwlr_layer_shell_v1 … DropTerm cannot
  start here; see {UNSUPPORTED_DOCS_URL}", the guard prints it to
  stderr and exits 1 (`dropterm/src/startup.rs:96–105`). The mac stub
  (`startup.rs:69–88`) is the same shape with the platform named and
  the successor module as the remedy: "macOS is not yet supported — no
  surface backend exists here until the macos-surface module lands;
  see https://l1br3-io.github.io/dropterm/known-issues/", pinned by a
  unit test (`startup.rs`, `the_macos_arm_refuses_naming_the_platform_
  and_the_successor` — asserted: contains "macOS", "macos-surface",
  and the docs URL). The docs row
  (`docs/known-issues.md:21`) declares itself "the refusal marker the
  surface module removes".
- **The external shape precedent: refusal plus named remedy.**
  rustup's component-missing error (source, `src/cli/rustup_mode.rs`,
  found by code search 2026-11-21) is verbatim "…is not installed for
  the toolchain '…'. help: run `rustup component add …` to install
  it" — a non-zero refusal whose message names the action that fixes
  it. The mac stub's "until the macos-surface module lands" is the
  same grammar with the future module as the remedy. The exit-status
  half needs no peer at all: exit 0 on success / non-zero on failure
  is the POSIX convention every surveyed tool follows.
- **The `--is-visible` honest `None` matches the query-verb
  convention.** `main.rs:171–179` answers `None` on non-Linux (exit 1,
  prints "false") without querying anything — the same contract the
  layer-shell guard already gives an unsupported Wayland session
  (main.rs's own comment calls the ordering "kept"). The closest
  external convention: `systemctl is-active` "Returns an exit code 0
  if at least one is active, or non-zero otherwise", still printing
  the observed state ("inactive") to stdout unless `--quiet`
  (freedesktop systemctl(1), version 262, fetched 2026-11-21) — a
  query verb answers with its state and a non-zero exit when the
  positive case cannot hold. Nothing surveyed lies to satisfy a query;
  the old behavior the guard replaced ("answer `--is-visible` with
  true" while never surfacing — known-issues.md's own history) is the
  anti-pattern this avoids repeating.
- The strongest alternative — refusing at `clap` level with a hard
  error before any verb — was implicitly rejected by the repo's own
  design: `--version` printing and `--is-visible` truthfully answering
  are real functions of a stub binary (CI scripts, package checks),
  and the guard is where the repo already centralizes refusal. No
  external precedent favors the harder refusal.

### 4. D4 — zbus compiled on mac as dead weight (adopted)

- **macOS is a supported zbus target, not a tolerated one.** The
  zbus README's portability statement (dbus2/zbus, master,
  2026-11-21): "Supported targets include Unix, Windows and macOS with
  Linux as the main target, and WASI." The 5.19 line in the lock
  compiles and its p2p wire tests pass on the mac target — re-verified
  on this host, where the full `cargo +1.98.0 test --locked --lib`
  run (1113 tests, 0 failures) includes the transport suite.
- **The cost is compile time and binary size on a target that ships
  nothing.** There is no `.app`, no DMG, no signing (the ruling in
  `docs/development.md`'s new macOS section: "build from source, that
  is the ruling"), so the dead weight lands in a `target/` directory.
  The mac graph (1417 lines) already carries the apple backend's size;
  zbus is a marginal addition.
- **The evidence value is real, not rhetorical.** The spec's decision
  (commit 51c5919) kept the D-Bus half compiled precisely because
  passing wire tests on mac are the proof that the later
  local-socket-transport replacement has somewhere to stand. Gating
  zbus now would delete that evidence and add a feature knob
  (`[features]` plumbing through `main.rs`'s `boot_frontend` split)
  whose only payoff is compile time on a development-only target.
- The strongest alternative — gate zbus behind a default feature —
  buys compile-time savings that no one consumes today and costs the
  mac-verified transport suite. Rejected. The revisit trigger is
  macos-surface's transport replacement, which removes zbus from the
  mac graph by design, not by gate.

### 5. D5 — the MSRV situation: CI pin is the only mechanism that exists (adopted)

- **`rust-version` is one value per package, all targets.** The Cargo
  Book's Rust-version chapter: the field "affects all Cargo targets in
  the package, including binaries, examples, test suites, benchmarks,
  etc." (doc.rust-lang.org/cargo/reference/rust-version.html,
  2026-11-21). Nothing in the manifest grammar can scope it per
  platform — there is no per-target MSRV to declare.
- **Cargo enforces dependency `rust-version` at resolution, with a
  clean error.** Tested on this machine (cargo 1.96.0, scratch package
  `/tmp/msrv-test`): a dependency declaring `rust-version = "1.99"`
  fails resolution with `error: rustc 1.96.0 is not supported by the
  following package: msrv-pkg@0.1.0 requires rustc 1.99` — before any
  compilation. The same experiment on the workspace's own package
  (adding `rust-version = "1.98"` to the scratch app) produced the
  resolution-time error listing the package itself, and
  `--ignore-rust-version` compiled cleanly — the opt-out the Cargo
  Book documents. Therefore: **bumping dropterm's `rust-version` from
  1.93 to 1.98 would break the ubuntu 1.97 CI pin** (dtolnay/rust-toolchain@1.97.0,
  `.github/workflows/ci.yml:64`) at cargo's own check, not at compile.
- **The pin's target really does need 1.98 — reproduced.** On this
  host (rustc 1.96.0), `cargo check --locked` fails with
  `error[E0658]: use of unstable library feature
  'result_option_map_or_default'` at
  `~/.cargo/registry/src/…/gpui-pre-macos-0.3.12/src/window.rs:896,
  957, 1068`, pointing at rust-lang/rust#138099. The 1.98.0 release
  notes list `Option::map_or_default` and `Result::map_or_default` in
  "Stabilized APIs" (rust-lang/rust RELEASES.md, 1.98.0 section,
  fetched 2026-11-21). `cargo +1.98.0 test --locked --lib` then passed
  clean on the same host. So the failure mode the CI comment warns
  about is real and was reproduced — with one correction: the API
  lives in **gpui-pre-macos** (the macOS backend crate), not
  gpui-pre-apple as the `.github/workflows/ci.yml:89–91` and justfile
  comments say.
- **Why resolution did not catch it: the pin's crates declare no
  `rust-version` at all.** Local registry grep across
  `gpui-pre{,-apple,-macos,-platform}-0.3.12` manifests: zero
  `rust-version` keys. The resolver's MSRV fallback (resolver 3's
  "incompatible-rust-versions = fallback" default) can only choose
  among versions a requirement admits — with the `=0.3.12` exact pin,
  there is one candidate and no declared floor, so cargo's documented
  behavior applies: "the resolver won't error but will instead pick a
  version, even if its potentially suboptimal" (resolver page), and
  the mismatch surfaces mid-compile as E0658 — exactly what this host
  showed. The inference the question asked for holds, doubly: by
  absent manifest key *and* by observed failure mode.
- **Established handling of a target-specific std-API floor: CI pin +
  docs, because the manifest cannot express it.** The repo's shape
  (mac job at 1.98, ubuntu at 1.97, `rust-version = "1.93"` kept as
  the true all-platform floor, `just deps` Darwin arm enforcing the
  1.98 floor with the check-first `xcrun --find metal` pattern,
  `docs/development.md`'s macOS section stating the floor) is the only
  mechanism that survives the manifest's constraints. The alternative
  — one rust-version for all — is worse in both directions: 1.98
  breaks ubuntu needlessly (the wayland backend the ubuntu job
  compiles never touches the mac-only API), and staying at 1.93 hides
  the real floor from mac developers until E0658. Adopted as-is.

### 6. D6 — Metal Toolchain on CI (adopted; the step is a flakiness guard, not the primary fix)

- **macos-latest is macOS 26 arm64 with Xcode 26-era tooling**
  (actions/runner-images README, fetched 2026-11-21: "macos-latest,
  `macos-26` or `macos-26-xlarge`" → macOS 26 Arm64).
- **The Metal toolchain split is Apple's doing, and the workaround
  command is Apple's own.** Apple documents Metal Toolchain under
  Xcode → Settings → Components ("Downloading and installing
  additional Xcode components", developer.apple.com, 2026-11-21), and
  the compiler's own error — quoted verbatim in runner-images issue
  #13014 — names the command: "error: cannot execute tool 'metal' due
  to missing Metal Toolchain; use: xcodebuild -downloadComponent
  MetalToolchain". Xcode 26 shipped without it bundled; runner-images
  #13080 reports the ~700 MB component "is an optional download"
  since Xcode 26.
- **The images preinstall it for release Xcode 26+, by policy.**
  Runner-images maintainer (erik-bershel) in #13080 (closed
  2025-09-27): "Image updates rollout complete… We plan to provide
  this component for all release versions of Xcode 26 and beyond. For
  beta versions and release candidates, you will need to install the
  component manually."
- **So the CI step's correct reading: belt-and-braces against a
  documented flake.** Issue #14013 ("MetalToolchain component
  occasionally missing on macos-26 runners", closed 2026-05-06)
  records real intermittent misses tied to which Xcode is selected.
  The mb-4 step (`xcodebuild -downloadComponent MetalToolchain || true`)
  is a best-effort pre-warm that cannot fail the job — the build
  itself is the gate, and the justfile's local recipe
  (`just deps` Darwin arm) does the more careful check-first version
  (`xcrun --find metal` before downloading). On today's images the CI
  step is most likely a no-op; its cost is the `|| true` masking a
  genuinely missing toolchain until `cargo build`'s shader compile
  fails with the Apple error above — acceptable, since the failure is
  diagnostic and the fix is one rerun. Adopted; if image flakiness
  ever shows in job timing, switch the CI step to the justfile's
  check-first shape.

## What DropTerm has today

- The branch `port/macos-build` (commits `6edfff5..bcc3a8c`) compiles
  and passes its full lib test suite on a real macOS arm64 host under
  rustc 1.98.0: 1113 passed / 0 failed / 0 ignored (run on this
  machine, 2026-11-21). Under the host's default rustc 1.96.0 the
  build fails at E0658 in `gpui-pre-macos` — the exact failure the
  1.98 CI pin and `just deps`' floor exist to prevent.
- Linux-only surface deps ride target tables (`dropterm/Cargo.toml:
  152`); the mac target resolves zero wayland packages (cargo tree,
  this host). Cargo.lock is untouched by the branch.
- The frontend gates Linux-only at module/function level with paired
  non-Linux arms: `main.rs:35–57` (six modules + `boot_frontend`
  extraction at `:199`), `startup.rs:63–87`, `terminal/clipboard.rs`
  (five functions), `terminal/view.rs` (shell field, `adopt_shell`,
  split `dispatch_shell_verb`), `engine.rs:4423` (`ssh_destination`).
- The mac boot refuses: `startup_verdict`'s non-Linux arm errors with
  the platform + successor-module message (`startup.rs:69–88`), pinned
  by a mac-side unit test; `--is-visible` answers the honest `None`
  (`main.rs:171–179`); the refusal is documented as the removable
  marker in `docs/known-issues.md:21`.
- zbus 5.19 stays compiled on mac (commit 51c5919) — supported target
  per upstream, wire tests pass on mac (in the 1113).
- CI gained the `macos-latest` job (`.github/workflows/ci.yml:84–119`:
  1.98.0, build + test arm64, `cargo check --target
  x86_64-apple-darwin`, best-effort Metal Toolchain); the ubuntu job
  keeps its 1.97.0 pin (`:64`); the crate manifest keeps
  `rust-version = "1.93"`; `just deps` grew a Darwin arm verifying
  Xcode CLT, Metal Toolchain (check-first), and the 1.98 floor;
  `docs/development.md` documents the whole mac build path.
- Seventeen Linux-only engine/pane tests carry per-test `cfg(target_os
  = "linux")` gates with reasons (`core/terminal/engine/tests.rs`,
  `terminal/view.rs` tests) — live-/proc reads, the dash-prompt
  `/bin/sh` harness family (macOS `/bin/sh` is bash 3.2, prompt
  `sh-3.2$ `), the non-UTF-8 fixture APFS cannot create, and the two
  live-pane gpui tests whose test scheduler aborts on the mac target
  (the repo already documents one `#[ignore]`d Linux test for the same
  thread-affinity limitation).

## Recommendation

Keep the module as shipped. Per decision:

| Decision | Verdict | Strongest alternative | Why not |
|---|---|---|---|
| D1 target tables | Adopted, with a corrected rationale | cargo feature `wayland` | Adds a user-facing knob a bin crate must never expose; `cfg(feature)` cannot drive target tables (Cargo Book); tables are what zed's gpui_platform uses |
| D2 paired cfg arms | Adopted, with a binding timing rule | Introduce the `SurfaceBackend` trait now | One impl, no joint; all three peers (alacritty, gpui, wezterm) paid for the seam only when the second backend landed — but the seam must land *with* macos-surface's first commit |
| D3 refusal stub | Adopted | Harder clap-level refusal before any verb | Kills `--version`/`--is-visible` truthfulness; the guard is the repo's existing refusal home; rustup/systemctl precedent favors refuse-with-remedy |
| D4 zbus un-gated | Adopted | Feature-gate zbus now | macOS is a supported zbus target; the wire tests are the portability evidence the later transport replacement stands on; nothing ships on mac to pay the dead weight |
| D5 CI pin 1.98 | Adopted | Bump crate `rust-version` to 1.98 | Verified experimentally: breaks the ubuntu 1.97 pin at cargo's resolution check; no per-target MSRV exists in the manifest grammar |
| D6 Metal step | Adopted | Drop the step (images preinstall since 2025-09) | Documented flakiness (#14013); the `|| true` pre-warm cannot fail the job and the build remains the gate |

Two corrections owed to the comments, not the code: the mb-1
rationale (wayland-sys pkg-config on mac) is not what the feature
graphs do — the gate's true justifications are honest scope, the
smaller mac graph, and the `client_system`-without-`dlopen` trap
guard — and the `map_or_default` crate is `gpui-pre-macos`, not
`gpui-pre-apple` (`.github/workflows/ci.yml:89–91`, the justfile's
Darwin comment). Comment-level, non-blocking; fix opportunistically.

## What would change these verdicts

- **macos-surface landing**: replaces D3's refusal and D2's arms —
  the `SurfaceBackend` seam must be that module's first structural
  commit, or D2's verdict flips to "reconsider" (arms accreting past
  one backend is the failure mode every peer avoided).
- **A dependency enabling `wayland-backend/client_system` without
  `dlopen` reaching the mac graph**: the mb-1 gate then becomes
  load-bearing for the reason the comment claims — today it is a
  guard, not a fix; ashpd's wayland feature is the named risk
  (it enables `client_system` in the Linux graph).
- **gpui-pre bump**: a new snapshot changes the std-API floor and the
  Metal-shader story in one move — re-run D5's E0658 check and D6's
  toolchain facts at each bump; the kit-root-border record's per-bump
  checklist is the template.
- **mac becoming shippable** (`.app`/DMG): D4's dead-weight calculus
  inverts — gate or replace zbus when a mac artifact carries it to
  users, which the transport-replacement module already schedules.
- **runner-images policy change** (Metal Toolchain removed from
  release images, or a beta Xcode pinned): D6's step graduates from
  guard to requirement; the check-first CI shape would be owed then.
- **A published `rust-version` on the gpui-pre crates ≥ 1.98**: D5's
  E0658 failure mode becomes a clean resolution error on any
  toolchain, including ubuntu's — the CI pin would then need a
  matching ubuntu bump or a vendored old-version pin; watch the
  manifests at each bump.

## Sources

Fetched 2026-11-21 unless noted; upstream master files cited as of
that date.

- doc.rust-lang.org/cargo/reference/specifying-dependencies.html —
  platform-specific dependency tables verbatim, the
  `cfg(feature)` prohibition, build-dependency target note.
- doc.rust-lang.org/cargo/reference/resolver.html — resolver versions
  1/2/3 differences, the v2 target-specific feature-unification rule
  (verbatim example), the fallback behavior ("won't error but will
  instead pick a version").
- doc.rust-lang.org/cargo/reference/manifest.html +
  /reference/rust-version.html — rust-version semantics: error on
  unsupported toolchain, all-Cargo-targets scope, `--ignore-rust-version`
  opt-out, MSRV respected since 1.56.
- github.com/zed-industries/zed — `crates/gpui_platform/Cargo.toml`
  (target tables selecting gpui_macos/gpui_linux/gpui_windows/gpui_web),
  `crates/gpui/src/platform.rs` (cfg gates on methods, the `Platform`
  trait as seam), the gpui_platform extraction commit
  (#49277, 2026-02-19, read via the repo's commit history for the
  path).
- github.com/alacritty/alacritty —
  `alacritty_terminal/src/tty/mod.rs` (master): the
  `#[cfg(not(windows))] mod unix;` / `#[cfg(windows)] pub mod
  windows;` split and the `EventedReadWrite`/`EventedPty` traits.
- github.com/wezterm/wezterm — `pty/src/lib.rs` (master): `#[cfg(unix)]
  pub mod unix;` / `#[cfg(windows)] pub mod win;`, the `PtySystem`
  trait, `native_pty_system()` runtime dispatch.
- github.com/rust-lang/rust — `RELEASES.md` 1.98.0 "Stabilized APIs":
  `Option::map_or_default`, `Result::map_or_default`; issue #138099
  referenced by the E0658 diagnostic itself.
- github.com/rust-lang/rustup — `src/cli/rustup_mode.rs` (found via
  GitHub code search): the "is not installed for the toolchain …
  help: run `rustup component add …`" refusal shape.
- github.com/dbus2/zbus — README portability section: "Supported
  targets include Unix, Windows and macOS with Linux as the main
  target, and WASI."
- github.com/actions/runner-images — README (macos-latest → macOS 26
  Arm64; macOS 14 retired 2026-11-02) and issues #13014 (open,
  2025-09-13, Apple's error text), #13080 (closed 2025-09-27, the
  maintainer's preinstall policy quote), #13094 (dup), #14013 (closed
  2026-05-06, intermittent misses, Xcode-selection tie).
- developer.apple.com/documentation/xcode/downloading-and-installing-
  additional-xcode-components — Metal Toolchain as an Xcode
  "Other Component".
- www.freedesktop.org/software/systemd/man/latest/systemctl.html —
  is-active/is-enabled exit-code semantics, state printed to stdout.
- Local registry (`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`):
  `gpui-pre-{0.3.12,apple-0.3.12,macos-0.3.12,platform-0.3.12}/
  Cargo.toml` (no rust-version keys; platform target tables),
  `gpui-pre-macos-0.3.12/src/window.rs:896,957,1068` (the
  `map_or_default` sites), `wayland-sys-0.31.11/build.rs` (the
  dlopen-first early return, the `client`-feature pkg-config probe),
  `wayland-backend-0.3.17/Cargo.toml` (client_system → wayland-sys/
  client), `wl-clipboard-rs-0.9.5/Cargo.toml` (no default features).
- This repo: `git diff main..HEAD` (commits `6edfff5..bcc3a8c`),
  `dropterm/Cargo.toml`, `dropterm/src/{main.rs,startup.rs,
  terminal/clipboard.rs,terminal/view.rs,core/transport.rs,
  core/terminal/engine.rs,core/terminal/engine/tests.rs}`,
  `.github/workflows/ci.yml`, `justfile`, `docs/development.md`,
  `docs/known-issues.md`, the workspace root `Cargo.toml`
  (`resolver = "3"`).
- Experiments on this host (macOS arm64, 2026-11-21): `cargo tree
  --locked --target {x86_64-unknown-linux-gnu,x86_64-apple-darwin}`
  (zero wayland packages on mac; 1673 vs 1417 lines);
  `/tmp/wayland-probe{,2}` (wayland-client and wl-clipboard-rs compile
  on macOS with default features); `/tmp/msrv-test` (cargo 1.96.0
  resolution-time `rust-version` errors for a dep and for the own
  package; `--ignore-rust-version` builds);
  `cargo check --locked` under 1.96.0 (E0658 reproduction) and
  `cargo +1.98.0 test --locked --lib` (1113 passed).

## Unverified

- **The ubuntu job's "1.99's clippy would flag pre-existing code"
  note** (`ci.yml:59`) is the repo's own pre-existing claim; this
  record did not run clippy 1.99 and neither asserts nor disputes it.
- **The un-gated mac counterfactual "would most likely have compiled"**
  rests on the two scratch probes and the feature-graph analysis; it
  was not run against the actual dropterm manifest un-gated (doing so
  would have modified the repo). The probes cover the deps' default
  feature sets; a full-tree counterfactual could still trip on
  something the probes do not exercise.
- **Exactly which crate enables wayland-backend's `dlopen` alongside
  ashpd's `client_system`** in the Linux graph was located to the
  ashpd subtree but not attribute-pinned to one ashpd feature;
  immaterial to the verdicts (both features are present either way,
  so the build script early-returns in both graphs).
- **The macos-latest image's exact preinstalled Xcode version at fetch
  date** was taken from the runner-images README's label table, not
  from a specific image build manifest; the Metal Toolchain
  preinstall policy (#13080) is the operative fact either way.
- **CI pass state of the new macos job** was not observed (no run was
  triggered from here); the local 1113-test run on matching rustc is
  the stand-in evidence for the build/test halves.
