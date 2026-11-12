# SPEC: distro-reach — what "works on other distros" ships as

Status: not started — owner rulings first · Phase 1 · lane C · branch
`port/distro-reach` (main checkout)

## Objective

Decide and deliver the install story beyond the Ubuntu-built tarball, from
`reality-check`'s distro session: the glibc floor (2.39 today), any missing
runtime libraries or fonts (`Noto Sans Mono`), and which native packages to
build.

## Dependencies

`reality-check`'s distro session (the floor and library facts);
`app-identity` merged (names what ships).

## Scope

Set by the rulings below. Touches `.github/workflows/release.yml` (shared
with `app-identity`'s rename of the tarball name), README's install
section, and whatever recipes the chosen channels need.

## Commands

    just check
    # release-channel recipes TBD by the rulings

## Open questions (owner rulings — this spec is a shell until they land)

1. Lower the glibc floor (build the release binaries on an older base) or
   document it?
2. Which native channels to build — the packaging record named AUR first,
   with `cosmic-ext-quake-terminal` as the template — and where their
   recipes live.

## Boundaries

- Always: README's install section states the floor whatever the ruling.
- Ask first: everything here is ask-first by nature — the rulings precede
  the chunk.
- Never: publish or push packages (outward-facing, the owner's hands; the
  release skill governs); commit `SPEC*.md` or `tasks/`.

## Success criteria

Set by the rulings; at minimum README's install section states the floor,
and the notes of the release that changes it say so.

## Size

M.
