# SPEC: reality-check — does today's binary work off COSMIC?

Status: not started · Phase 0 · lane: hardware — no code, no branch, no
worktree

## Objective

Learn what the shipped binary does outside COSMIC before any portability
code is written. Green results on sway, Hyprland, and KDE collapse "other
distros" into packaging and docs; every failure becomes a Phase 1 bug
package. This is kill-risk 1 in the plan's assumptions.

## Scope

In: the seven sessions and the sixteen per-session checks of the plan's
"Phase 0: the checklist" section (committed and canonical — run it from
there, not from this spec).

Out: any code change. A failure is *recorded*, then filed as its own bug
package or fixed in a commit named in the session's slot.

## Commands

    just build-release        # fallback where a session's glibc is older than 2.39
    RUST_LOG=debug cosmic-ext-dropterm   # the binary name until app-identity lands

Sessions run the v0.4.0 release tarball (what users install) by default;
VMs are acceptable except where the checklist says otherwise. Capture logs
per session.

## Deliverable

Results land in the pre-made per-session slots under "Phase 0 results" in
the plan record — on main, per the recording template: date, distro,
compositor and version, session type, output scales, the build used, a
`pass`/`fail`/`n-a` plus one line per check, and a log excerpt for every
failure. The slots are pre-made so parallel sessions never touch the same
lines.

## Testing strategy

The checks are the tests (core = 1–7 and 12–13; GNOME and X11 failures are
expected findings and do not count against the gate). No unit tests; no
code to test.

## Boundaries

- Always: record distro, compositor and version, session type, and each
  output's scale per session; file every failure as a bug package or a fix
  commit, named in the session's slot.
- Ask first: nothing — this package changes no repo state beyond the plan's
  result slots.
- Never: edit any part of the plan record beyond your session's slot; edit
  archived research records.

## Success criteria

- Every session slot under "Phase 0 results" is filled per the recording
  template.
- Each failure is filed as a bug package or fixed in a commit, named in the
  session's slot.

## Feeds

`layer-shell-guard` (how GNOME and X11 fail), `keybind-docs` (which
bindings work where), `distro-reach` (the glibc floor, missing libraries
and fonts), and new bug packages. Shares the sway session with
`gpui-spike`.

## Open questions

None blocking. Which X11 session runs (Xfce, Cinnamon, or i3) is a
recording choice, not a ruling.

## Size

Seven sessions × sixteen checks; manual.
