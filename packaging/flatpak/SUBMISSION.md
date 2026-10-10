# Submitting the DropTerm applet to pop-os/cosmic-flatpak

The maintainer's kit for the submission PR. Their repo enforces a PR
checklist (`.github/PULL_REQUEST_TEMPLATE.md` + `require-checklist.yml` CI);
this file maps every item to concrete content or evidence, lists the
exact files the PR adds, and records the local validation. The PR
**waits until the hardware walk below is filled in** — "tested and
working" is an enforced claim.

## The exact PR files

PR target: `pop-os/cosmic-flatpak`, branch off `master`, adding one
directory:

```
app/io.github.l1br3_io.CosmicExtDropterm.Applet/
├── io.github.l1br3_io.CosmicExtDropterm.Applet.json   ← copy of this repo's packaging/flatpak/<id>.json, verbatim
└── cargo-sources.json                                 ← regenerated: just applet-flatpak-sources (same generator, same Cargo.lock at v0.1.0)
```

This repo's manifest is the source of truth; their repo gets a copy.
If their review amends the copy, resync it back here (spec boundary).
Their CI builds every changed manifest with
`flatpak-builder --sandbox --install-deps-from=flathub` — the local
`--user` build proved the same manifest; their pass is the `--sandbox`
proof (OQ-3).

## Their enforced checklist, item by item

### 1. COSMIC trademark policy read and followed

Policy: [pop-os/cosmic-epoch TRADEMARK.md](https://github.com/pop-os/cosmic-epoch/blob/master/TRADEMARK.md).
DropTerm's compliance, point by point:

- Package namespace `cosmic-ext-dropterm` — §5.2's recommended
  third-party prefix (the plain `cosmic-` namespace is reserved for
  official software).
- App ID `io.github.l1br3_io.CosmicExtDropterm(.Applet)` — reverse-DNS
  under our own GitHub domain; `com.system76.*` is reserved and
  avoided.
- Phrasing: "for the COSMIC™ desktop" with the ™ marker (desktop-file
  Comment, metainfo description, README) — the exact §5.2 wording
  third parties are permitted.
- No COSMIC/Pop!_OS logos or artwork; the applet icon is our own SVG.

### 2. No LLM-generated content beyond your own app code/manifest

The manifest and the applet source are the project's own. The PR
**title and description must be human-written** — use this kit's
*facts* (checklist mapping, evidence, design notes below), not its
prose, and write the PR text in your own words.

### 3. DCO (Developer Certificate of Origin)

Sign off every commit in the PR: `git commit -s` (or `git commit
--amend -s` before pushing). The sign-off must match the commits'
author identity.

### 4. Tested and working

Evidence block — attach/reference in the PR:

- **Local flatpak build**: `just applet-flatpak-build` green from a
  clean `.flatpak-builder/` on 2026-10-10 (manifest at `4d8c256`;
  deps from flathub, `--user`, flatpak-builder 1.4.9):
  build → export → install all clean. No extra modules beyond the
  template; no sandbox opt-outs.
- **Export verification**: `io.github.l1br3_io.CosmicExtDropterm.Applet.desktop`
  under `~/.local/share/flatpak/exports/share/applications/` with
  `X-CosmicApplet=true` / `X-CosmicShrinkable=true` intact and `Exec`
  rewritten to `flatpak run … --command=cosmic-ext-dropterm-applet`;
  the `.Applet-symbolic` icon exported alongside.
- **Hardware walk**: **PENDING** — the three rows in
  `docs/development.md` (panel listing/themed render; sandbox
  boundary — flatpaked applet driving the native v0.1.0 app with
  indicator tracking; panel-restart respawn). Fill in per-row results
  and the date here before filing:
  - Listing and add: ______
  - Sandbox boundary: ______
  - Respawn: ______
  - Walked on: ______

## Design notes reviewers may ask about

- **`--talk-name=io.github.l1br3_io.CosmicExtDropterm`** — the one
  addition beyond the cappsy template. The applet calls the *host*
  app's bus name (toggle/show/hide verbs and the running/not
  `NameOwnerChanged` watch ride the same grant). The app itself is
  native-only: a sandboxed layer-shell client gets no layer shell on
  cosmic-comp, so the terminal ships from the GitHub release, never
  as a Flatpak.
- **Icon under two names in `/app`** — flatpak exports only icons
  named `<flatpak-id>*`, but the applet's own rendering (and the
  native install) use the id *without* the `.Applet` suffix. The
  manifest installs the same SVG under both names and rewrites the
  sandbox desktop file's `Icon=` to the exported one; nothing outside
  the flatpak changes.
- **`--talk-name=com.system76.CosmicSettingsDaemon`** — part of the
  cappsy template set (theme hot-reload), kept as-is.

## Filing sequence

1. Complete the hardware walk (fills the PENDING block above).
2. Fork/clone `pop-os/cosmic-flatpak`; branch; add the two files
   (regenerate `cargo-sources.json` fresh: `just applet-flatpak-sources`).
3. `git commit -s`, push, open the PR against `master`.
4. Their CI builds the manifest in `--sandbox` mode — green closes
   OQ-3.
5. Record the PR link in `SPEC.md`; resync this repo's manifest if
   review amends it.
