# The D-Bus contract

Everything an integrator needs to drive or query DropTerm from another program - a GNOME Shell extension, a KDE widget, a swaybar module - without reading Rust: the address, the two interfaces, the CLI's wire encoding, and the stability promise. Every member named below is pinned to the served surface by a wire test in the terminal's own suite: a member added to either interface without a row on this page fails `just check`.

## The address

All of it lives on the user's session bus, at one object path, owned by the running instance:

- **Bus name**: `io.github.l1br3_io.dropterm`
- **Object path**: `/io/github/l1br3_io/dropterm`
- **Interfaces**: `io.github.l1br3_io.dropterm.V1` (state) and `org.freedesktop.DbusActivation` (verbs), both served at that path, beside the standard `org.freedesktop.DBus` interfaces every object server adds (`Introspectable`, `Peer`, `Properties`)

The bus name is owned, not queued for: the first launch owns it for its lifetime, a second launch forwards through it and exits, and a name owned by a stale instance that no longer answers refuses the launch rather than spawning a duplicate. Development builds (`just dev`, the `dev-identity` feature - [Development](development.md#development-launcher)) answer at `io.github.l1br3_io.dropterm.Dev` and `/io/github/l1br3_io/dropterm/Dev` instead, so a dev build never collides with an installed one.

## `io.github.l1br3_io.dropterm.V1` - the state interface

Read-only glance state: two properties, no methods. This is the surface to watch for an icon, a widget, a status line.

| Member | Signature | Meaning |
|---|---|---|
| `Visible` | `b`, read | `true` while the drop-down surface is shown or animating in; `false` once it is gone or animating out. Exactly one flip per show and per hide. |
| `TabCount` | `u`, read | Number of open tabs. |

Changes arrive as the standard `org.freedesktop.DBus.Properties.PropertiesChanged` signal for the interface - emitted only when a value actually changes, so a no-op edit on the app side emits nothing. Reading a property is the standard `Properties.Get` call; that is all `dropterm --is-visible` does ([CLI reference](cli.md#querying-visibility)).

## `org.freedesktop.DbusActivation` - the verb interface

What a second launch - or your program, standing in for one - calls to make DropTerm act. Three methods, no properties, no reply payload:

| Member | Signature | Meaning |
|---|---|---|
| `Activate` | `(a{sv} platform_data) → ()` | Summon with no verb; the instance applies its default (a toggle). |
| `Open` | `(as uris, a{sv} platform_data) → ()` | Carried and ignored: DropTerm defines no openable target. Served for contract completeness - call it, get a normal reply, nothing happens. |
| `ActivateAction` | `(s action_name, as args, a{sv} platform_data) → ()` | Run a verb: `action_name` is the CLI verb's own name, `args` its argument words (the table below). The receiving instance re-parses both through the same definitions the CLI uses. |

The `a{sv} platform_data` dictionary (activation token, startup id) is accepted on the wire and dropped: DropTerm's surface is a layer surface, not the main window that token handling would unminimize.

Every member is fire-and-forget, and that is the contract: the call always replies `Ok` - even when the verb's queue is full (bounded at 64 pending verbs; overflow is dropped with a server-side warning) or the receiver side has died. An `Ok` reply means the running instance's server accepted the call, never that the verb was applied - if you need to observe the effect, watch `Visible`/`TabCount`.

The authority model is the CLI's own: any process in the same session can drive every verb, including spawning a command through `new-tab --cmd`. That is exactly the authority a `dropterm --cmd ...` launch has always had; the seam's trust boundary is the session bus ([Architecture](architecture.md)).

## The CLI verbs over the wire

Each CLI verb is one `ActivateAction` call - the verb's own name plus its arguments as plain words, only the flags actually given. A bare `dropterm` is no exception: it sends `ActivateAction("toggle", [])`. The plain `Activate` member is served for the contract's completeness (a desktop-activation style caller), but DropTerm's own CLI never sends it.

| CLI | Wire call |
|---|---|
| `dropterm` / `dropterm toggle` | `ActivateAction("toggle", [])` |
| `dropterm show` | `ActivateAction("show", [])` |
| `dropterm hide` | `ActivateAction("hide", [])` |
| `dropterm new-tab [--cwd DIR] [--cmd COMMAND] [--profile NAME]` | `ActivateAction("new-tab", ["--cwd", DIR, "--cmd", COMMAND, "--profile", NAME])` - only the flags given |
| `dropterm close-tab [--tab-index N]` | `ActivateAction("close-tab", ["--tab-index", N])` when the index is given, else no args |
| `dropterm select-tab N` | `ActivateAction("select-tab", [N])` |
| `dropterm rename-tab TITLE [--tab-index N]` | `ActivateAction("rename-tab", [TITLE, "--tab-index", N])` - TITLE first, always |
| `dropterm duplicate-tab [--tab-index N]` | `ActivateAction("duplicate-tab", ["--tab-index", N])` when given |
| `dropterm switch-profile-tab --profile NAME [--tab-index N]` | `ActivateAction("switch-profile-tab", ["--profile", NAME, "--tab-index", N])` |
| `dropterm --is-visible` | not a member - a `Properties.Get` of `Visible` on the V1 interface |

Full verb semantics (indices 1-based, quiet vs. showing verbs, what each does at startup) are the [CLI reference](cli.md)'s; the wire carries exactly those verbs, nothing separate.

Two `busctl` lines to try it:

```sh
# toggle the overlay
busctl --user call io.github.l1br3_io.dropterm /io/github/l1br3_io/dropterm \
  org.freedesktop.DbusActivation ActivateAction sasa{sv} "toggle" 0 0
# read the glance state
busctl --user get-property io.github.l1br3_io.dropterm /io/github/l1br3_io/dropterm \
  io.github.l1br3_io.dropterm.V1 Visible
```

## The stability promise

The plan's ruling, stated exactly: **the V1 D-Bus contract is *the* API; D-Bus is its Linux binding.** For an integrator that means the members documented here - their names, signatures, and the semantics above - are the stable surface: a newer DropTerm keeps answering an older caller across an upgrade, which is the same rule the second launch itself obeys. The served surface is pinned by wire tests (`just check`), so a member cannot change shape quietly. Nothing beyond that is promised: no versioning scheme, no pre-announced members.

## Writing a consumer

The CLI is the reference consumer: a second launch forwards its verb through `org.freedesktop.DBus` to the running instance, and the same one encoder/decoder pair (`cli.rs`) serves every word-carrying tab verb. An out-of-tree consumer needs only a D-Bus client; everything on this page is the whole contract.
