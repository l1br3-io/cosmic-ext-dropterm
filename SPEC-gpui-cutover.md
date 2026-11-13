# SPEC: gpui-cutover — the GPUI build becomes DropTerm

Status: not started · Phase 3 · lane: main (branch `port/gpui-cutover`,
after `gpui-parity`; every step beyond the code swap is owner-gated)

## Objective

The swap: the GPUI frontend becomes the default and only terminal binary —
`dropterm` — and the libcosmic frontend is deleted. Docs and packaging
follow the code; the deferred release (owner ruling, 2026-11-12: it carries
everything merged by then) is cut from the result, walk gate standing.

## Preconditions

`gpui-parity` merged — parity proven on COSMIC and sway is the gate to
touching the default.

## Scope

- The rename: the GPUI crate's bin becomes `dropterm`; the libcosmic
  frontend (app composition, executor, slide, widget, tab bar, status
  band, settings, context menu, grab bar, drawer handle, the clipboard
  anchor's remains) is deleted; `dropterm-core` keeps both its remaining
  consumers honest (the applet, and the terminal bin).
- The applet: untouched functionally — it talks the V1 contract through
  `dropterm-core` and its own libcosmic applet surface; its canary row
  re-walks.
- Packaging: the release workflow's binary set, desktop and metainfo files
  unchanged in identity (the app id never moved), the tarball contents
  updated to the new binary; `just` recipes reviewed.
- Docs: [architecture](../docs/architecture.md) rewritten to the
  one-core-two-bins-then-one reality; [known issues](../docs/known-issues.md)
  gains the carried fractional-scale row (geometry honored, no visible
  output on COSMIC at this pin; integer-rounded display metadata both
  compositors) if it is not there already; README and the configuration
  page checked for anything the swap invalidates.
- The release: version bump, notes covering everything since the last tag,
  the staged-notes merge, the owner-pass halves and post-publish tarball
  walk before the tag moves (`dropterm-release` ritual; nothing pushes
  itself).

## Commands

    just check && just build-all
    /release x.y.z    # the ritual; the owner fires it

## Testing strategy

`just check` green on the result; the applet canary; a final smoke walk of
the installed `dropterm` on the validation VM (toggle, applet, config
edit, session restore). The release's own walk gate is the ritual's.

## Boundaries

Never: delete or change the applet beyond its dependency re-point; change
the contract, config locations, or identity; cut the release without the
owner; delete the carried-patch copy or its ledger entry (the upgrade
ritual owns it until upstream ships the opt-out).

## Success criteria

One terminal binary, `dropterm`, GPUI-backed; the applet works against it;
`just check` green; docs true; the release cut carries the initiative.

## Size

M.
