# Styling

Plugins style DropTerm with CSS conventions: a selector-matched,
specificity-ordered cascade of declarations over the whole surface -
app chrome included. The model reads like CSS on purpose: element
names, classes, ids, specificity, source order - with DropTerm's
own fail-by-rule culture: one log line, the declaration drops,
nothing ever raises into the terminal.

## Declarations

A declaration is one kebab-case property with one value. The
vocabulary is closed; growth is additive only, each its own ruling.

| Property | Values | Paints |
|---|---|---|
| `color` | a theme token or `#rrggbb` | the text color |
| `background` | a theme token or `#rrggbb` | the element fill |
| `font-weight` | `bold`, `normal` | the glyph weight |
| `font-style` | `italic`, `normal` | the slant |
| `text-decoration` | `underline`, `line-through`, `none` | the decorations |
| `icon` | a glyph string or Lucide name | the glyph prepended before the text |

The color tokens are the scheme's own words: `accent` (the scheme's
BrightBlue), `fg` (the pane's foreground), `bg` (the worn scheme's
background - the one addition chrome rules need), and the ANSI words
- `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`,
`white`, and their `bright_black`, `bright_red`, `bright_green`,
`bright_yellow`, `bright_blue`, `bright_magenta`, `bright_cyan`,
`bright_white` forms. A token resolves at paint through the scheme -
a theme or scheme flip re-resolves every rule live. An unknown
property or value costs one `dropterm:` log line and drops that one
declaration; the rest of the rule stands. There is no `opacity` and
no script-set alpha anywhere - the dim story is a `color` rule,
nothing multiplies on top.

## Selectors

One compound selector: an optional element name leading, then
`.class` parts and one optional `#id`, in any order -
`tab.active`, `.warn`, `#main-meter`, `dialog-button.accent`.
Specificity is the CSS tuple (ids, then classes, then the element
name), inline declarations above all. There are no
`!important`, no pseudo-classes, no descendant or child combinators,
and no media queries - in v1 and, by the ruling's shape, likely ever
(a growth arrives as its own additive ruling).

## The cascade

All matching rules compete per property by specificity; equal
specificity breaks by source order, and source order is fixed:

1. the host's builtin layer (today's look as ordinary rules)
2. the manifest sheets, plugins in dir-name order
3. the runtime sheets, in call arrival order
4. the element's own inline `style` table, above everything

Unset properties inherit from the base layer - the scheme's value
for that surface; the band sides' builtin color flows into the
spans' unset colors exactly like CSS inheritance (color and the font
properties inherit; `background` and `icon` do not).

## The addressable tree

Every addressable node carries a stable element name; state and
placement ride classes.

| Selector matches | Names and classes |
|---|---|
| The status band | `band`, `band.left`, `band.right` |
| The tab strip | `tabstrip`; `tab` per tab, `tab.active` for the focused one |
| The settings window | `settings`; `settings-page` per page root, classed per page - the words `appearance`, `window`, `tabs`, `status-bar`, `dropdown`, `terminal`, `advanced`, `profiles`, `keybindings`, `plugins` (one rule per page, e.g. `settings-page.appearance`) |
| The modal surface | `dialog`; `dialog-button` per button |
| The right rail | `panel`; `panel-view` per docked view |
| Typed content | `span` (band, dialog body, tab-title spans), `row` (panel rows) |

The host's builtin layer carries today's look as ordinary rules -
the right cluster's dim is the `band.right` color, the primary
button's look the `dialog-button.accent` rule - so a plugin
out-specifying or out-ordering a builtin replaces it by ordinary
cascade. A panel row's numeric event id is not rule-addressable (a
digit-led `#id` does not parse); class rules address it.

## The two sources

**Manifest styles** - `plugin.json` grows one key, `"styles"`, a
flat array of rule objects; chrome restyles from launch with zero
interpreter, and rebuilds on every hot reload:

```json
{
  "styles": [
    { "selector": "tab.active", "declarations": { "color": "accent", "font-weight": "bold" } },
    { "selector": "settings-page.plugins", "declarations": { "background": "#1a1b26" } }
  ]
}
```

A rule object outside the `selector`/`declarations` vocabulary costs
one log line and is ignored; a rule with an empty selector, empty
declarations, or unparseable selector syntax drops with one line
naming it.

**The runtime verb** - one call replaces the caller's whole runtime
sheet (the status segment's replacement-seat discipline):

```lua
dropterm.styles({
  { selector = 'band.right', declarations = { color = 'bright_black' } },
  { selector = '.warn',      declarations = { color = 'red', icon = '⚠' } },
})
```

The sheet applies at call, repaints the surfaces it touches, and
dies with its interpreter on teardown - manifest sheets, if any,
then stand alone. A plugin with both sources runs both, its manifest
rules ordered before its runtime rules.

## Trust, stated plainly

A plugin can restyle app chrome - the status band, the tab strip,
the settings window, this page's whole tree. That is the feature,
not an accident: the same full-trust model every plugin surface
runs under ([the plugins page](plugins.md) states it once, at the
top). Styles never run code and never take the keyboard; they paint.
