# Chunk: layer-shell-guard — fail loudly without layer-shell

Spec: [SPEC-layer-shell-guard.md](../SPEC-layer-shell-guard.md) · Module
id: `layer-shell-guard` · Phase 1, lane C · branch
`port/layer-shell-guard` (main checkout, or `../ced` between
lane A's packages).

**Files touched:** `src/main.rs` (the probe at startup, before surface
creation; `--is-visible`'s truth), [known
issues](../docs/known-issues.md) (one row), plus a small new module for
the verdict. Shares `src/main.rs` with `control-transport` — land this
small guard first (plan guidance), or it rebases after.

## Task 1: The verdict function

**Description:** A pure decision function — global list in, verdict out
— so it is unit-testable headless: with `zwlr_layer_shell_v1` present,
the verdict is "supported" and nothing changes; absent, "unsupported
here" naming the missing protocol. This is the first slice of backend
detection (design rule 3, spike check 11): it reports where a later
build dispatches.

**Acceptance criteria:**
- [x] Unit tests cover both verdicts over representative global lists
- [x] The function is pure (no I/O)

**Verification:**
- [x] Tests pass: `just check` (commit f4d6a74)

**Dependencies:** `app-identity` merged (rebase onto it) · **Files:**
the new module · **Scope:** S

## Task 2: The startup probe (ask-first gate)

**Description:** Wire the verdict into startup: probe the Wayland
connection's registry for the layer-shell global before any surface is
created; on the unsupported verdict, print an error naming
`zwlr_layer_shell_v1`, link the docs, and exit non-zero. `--is-visible`
becomes truthful when no surface can exist (no silent true). The
probe's connection is the spec's ask-first item: a direct
`wayland-client` dependency for the registry roundtrip (transitive
today; making it direct is the ask) versus probing through an existing
dependency's connection — ask the owner at task start. Add the
[known issues](../docs/known-issues.md) row.

**Acceptance criteria:**
- [x] Under a compositor without layer-shell, the binary exits
      non-zero with the message naming the protocol and linking docs
      *(headless smoke of the no-Wayland variant passed at ed0f10e:
      message + exit 1; the compositor-without-layer-shell variant
      passed 2026-11-11 against weston 13.0.0 headless in a docker
      container, its socket shared to the host: exit 1 with the
      message and no probe reason — the roundtrip succeeded and the
      registry lacked the global; taken on best judgment after the
      ask to the owner went unanswered, docker pulls being an
      established local pattern via docs-serve. Not GNOME
      itself: weston satisfies the "or a nested compositor without
      layer-shell" branch in substance, over a headless socket rather
      than a nested one)*
- [x] `--is-visible` reports truthfully when no surface can exist
- [x] The known-issues row lands

**Verification:**
- [x] Tests pass: `just check` (commit ed0f10e)
- [x] Manual: GNOME (Phase 0 session 4's environment) or a nested
      compositor without layer-shell *(2026-11-11, weston 13.0.0
      headless over docker: `wayland-info` in the container listed 19
      globals with no `zwlr_layer_shell_v1`; the host-run dev image
      against that socket printed the startup error naming the
      protocol and linking the docs, exit 1; a control run before the
      socket was made writable exited 1 carrying a probe reason,
      showing the message distinguishes cannot-ask from asked-and-no;
      `--is-visible` under the same session answered false / exit 1
      with no guard message. Container, images, and socket dir
      removed afterward)*

**Dependencies:** Task 1 · **Files:** `src/main.rs`,
`docs/known-issues.md`, `Cargo.toml` if the direct dep is sanctioned ·
**Scope:** M

## Task 3: No-change verification and merge

**Description:** COSMIC and sway are unchanged (the global exists
there) — verify both, then merge. No fallback surface ever (a `PopUp`
path is Phase 4's backend, not this package's).

**Acceptance criteria:**
- [x] COSMIC and sway behave exactly as before the guard *(both halves
      re-verified 2026-11-11 on the final tree — the branch rebased
      onto app-identity's main first, the guard's message prefix and
      docs URL carrying the new identity; COSMIC on cosmic-comp: the
      full truth table over the dev identity (launch true/0, hide
      false/1, show true/0, toggle both ways, killed false/1), guard
      silent; sway as Debian trixie headless in docker (uid-matched
      user, socket shared to the host, `wayland-info` confirming
      zwlr_layer_shell_v1 v4 among 40+ globals): the identical truth
      table at the same ~4s fresh-launch serve latency, guard silent,
      session healthy after; the serve race itself measured identical
      with and without the guard via a main-built control binary
      (served at 4.152s on both) — the earlier interface-missing reads
      were that pre-existing race stretched by a concurrent just check,
      not a regression)*
- [x] `just check` green on the merge result *(f73cf27, exit 0; the
      /ship three-way review ran before it — no Critical/Important
      findings, its hold-worthy gap closed by the testing.md manual
      row 191dfe1 and the duplicate-globals pin 13116ff on the
      branch; two Low security notes — probe-reason sanitization,
      probe timeout — recorded in the merge message for follow-up)*

**Verification:**
- [x] Manual on COSMIC (and the sway session if at hand): normal
      launch, toggle, `--is-visible` *(2026-11-11, cosmic-comp, the
      dev-identity image at ed0f10e so the running release instance
      stayed untouched: first launch passed the guard and surfaced,
      `--is-visible` true / exit 0; a bare second launch hid it, false
      / exit 1; a third showed it, true / exit 0; with the instance
      killed, false / exit 1 and no guard message; empty stderr log.
      Evidence is the visibility truth table and exit codes, read from
      a shell, not an eyes-on check of the overlay. Sway: not
      installed here, not run)*

**Dependencies:** Task 2 · **Files:** the branch's diff · **Scope:** S
