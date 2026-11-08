# Experiment: does cosmic-comp grant layer shell to a flatpaked client?

The single-install cosmic-store goal (`packaging/flatpak/SUBMISSION.md` scope
note) turns on one question the research record currently answers by
assumption: can the sandboxed terminal create its layer-shell surface?
Modern flatpak passes the Wayland socket through unfiltered, so any gating
is cosmic-comp's. No layer-shell app exists in pop-os/cosmic-flatpak today
to cite as precedent, either way. This manifest answers it empirically.

The manifest (`io.github.l1br3_io.CosmicExtDropterm.json`) packages the
terminal binary from the same pinned v0.3.1 archive the applet uses. It is
an experiment, not a submission candidate: `--filesystem=host` and friends
make it a real terminal, but nothing here has passed a catalog review.

## Finish-args, and why they are broader than the applet's

- `--socket=wayland` — the surface under test
- `--device=dri` — wgpu rendering
- `--filesystem=host` — a terminal that cannot run the user's shell in
  their home proves nothing about the real product
- `--own-name` + `--talk-name` on the app's bus name — the first run owns
  the single-instance name, every later `flatpak run` talks to it; both
  grants are needed for the toggle path the shortcut binds

## The walk (COSMIC validation VM)

The host has `flatpak` but not `flatpak-builder`; the harness builds in
the guest (`docs/development.md` store walk, `scripts/vm.sh`). Run
against a snapshot with no native DropTerm running, so the surface's
origin is unambiguous.

1. Host: pair the manifest with a v0.3.1 `cargo-sources.json` — never
   regenerate from main's working tree (its lock carries the
   `[patch.crates-io]` forks and mismatches the pinned archive; the
   store-walk lesson). Either reuse the fork branch's recorded pair, or
   extract `Cargo.lock` from the pinned v0.3.1 tarball and run the
   generator (`just applet-flatpak-sources` recipe, manual invocation)
   against that lock.
2. Host: `just vm-put` the manifest and that `cargo-sources.json` into
   the guest (beside each other, manifest referencing
   `cargo-sources.json` by its basename).
3. Guest (one-time if absent): `apt install flatpak-builder`, flathub
   remote. Then `flatpak-builder --user --force-clean --install
   <dir> <manifest>` over the two files.
4. Guest terminal: `flatpak run io.github.l1br3_io.CosmicExtDropterm`.
5. Observe and record (`just vm-screenshot` for evidence either way):
   - PASS: the overlay slides down from the edge, above other windows,
     absent from Alt-Tab and the dock.
   - FAIL: no surface, a normal toplevel instead (shows in dock), or a
     protocol error at launch. Capture `journalctl --user -b` lines
     around the attempt.
6. While PASS holds: bind a COSMIC custom shortcut to
   `flatpak run io.github.l1br3_io.CosmicExtDropterm` and press it twice
   — second press must toggle, proving the single-instance seam inside
   the sandbox.
7. Cleanup: guest `flatpak uninstall --user io.github.l1br3_io.CosmicExtDropterm`.

The `just terminal-flatpak-*` recipes stay for any host that has
`flatpak-builder` (they mirror the applet trio); this walk uses the
guest route because that is where flatpak-builder lives.

## What each outcome means

- PASS: the single-install goal is reachable as one flatpak under the
  main app id — terminal plus applet binaries in one manifest, the applet
  launching its sibling directly. The catalog listing moves from the
  `.Applet` id to the main id; the applet submission PR is then
  reconsidered (superseded, or filed as the stopgap it already is).
- FAIL: single-install is blocked on cosmic-comp. The path is the
  upstream conversation (layer shell for sandboxed clients, or a
  sanctioned drop-down mechanism), the applet PR ships as scoped, and
  this manifest records the negative result.

Record the outcome in `docs/research/packaging-and-release.md` (status
row in `docs/research/README.md`) and keep this file as the experiment's
walk record either way.

## Outcome (2026-11-08): FAIL — cosmic-comp filters layer shell from sandboxed registries

Walked on the COSMIC validation VM (Pop!_OS 24.04, snapshot
`pre-layershell-exp`). Build green (4m 45s, guest flatpak-builder), app
installed and ran, no errors — and no surface, ever.

| Probe | Result |
|---|---|
| Terminal process inside sandbox | alive, empty error log |
| Wayland connection from sandbox | live; sctk bound wl_output/wl_seat/xdg_output/shm |
| Single-instance D-Bus seam | works; bus name owned, `--is-visible` answered, activation toggled state |
| Display (pixel diff vs baseline) | unchanged except the panel clock (140 px) |
| `wayland-info` from native session | 56 globals, `zwlr_layer_shell_v1` v5 advertised |
| `wayland-info` inside the sandbox (host binary via `/run/host`, `--filesystem=host`) | 33 globals, layer shell absent; `xdg_wm_base` v7 present |

Interpretation: cosmic-comp builds sandboxed clients a reduced registry
— normal windows (`xdg_wm_base`) survive, overlay-capable protocols do
not. That is a deliberate security boundary (a sandboxed layer surface
with keyboard interactivity is a UI-bound sandbox escape), and it is why
cosmic-edit flatpaks fine while a drop-down terminal cannot. The
mechanism (per-connection security context at the socket level) means
no finish-args combination can grant it; the single-install goal is
blocked on cosmic-comp exposing a sanctioned mechanism, not on packaging.

This confirms first-party what the research record
(`docs/research/packaging-and-release.md` §2.4) had already established
from [cosmic-flatpak
#259](https://github.com/pop-os/cosmic-flatpak/issues/259) and
cosmic-comp's binding code: layer shell binds only for clients without
a Wayland security context. The experiment's contribution is the direct
on-VM evidence for DropTerm's own pinned binary and manifest, not a new
fact about cosmic-comp. The open upstream question from #259 (a
sanctioned path for sandboxed layer-shell clients) remains the sole
lever for the single-install goal.

Secondary finding, app-side: DropTerm is silent about the missing
protocol. It reports `visible=true`, logs no error at default level, and
idles with "no terminal event channel yet; deferring terminal spawn"
forever. A native run on any compositor without wlr-layer-shell would
hang the same way. Worth its own issue: fail loudly (or fall back to a
normal toplevel under a flag) when layer shell is absent.

Evidence on the host: `sandbox-globals.txt`, `native-globals.txt`,
`dt-run3.log` (124 lines, RUST_LOG=debug), `baseline.ppm` and the
after-launch screenshots (byte-identical to baseline). Guest cleaned:
instance killed, experiment flatpak uninstalled, applet untouched.
Two harness fixes landed during the walk: `vm.sh` `cmd_put` no longer
leaves root-owned staging directories, and the guest runs commands
under `sh` (dash) — no bash-only syntax in `vm-exec-user` arguments.
