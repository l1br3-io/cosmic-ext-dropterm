# VS Code's extension-UI architecture, read from the shipped 1.147.0 artifact (REA-grounded)

**Question:** What does the shipped VS Code artifact prove first-hand about the
typed-slots extension-UI architecture DropTerm's plugin-UI plan adopts — the
model the owner ruled in on 2026-11-24 ("real VS Code: typed slots")?

**Date:** 2026-11-24

**TL;DR:** The shipped VS Code 1.147.0 artifact proves the typed-slots model
at every layer the plan copies: one identifier table maps string ids to
~90 typed `MainThread*` proxies, every extension-facing UI surface
(status item, webview view, tree view) is one entry in that table, the
extension host holds only proxies and never touches the workbench, the
host calls back into extension code through `$`-prefixed typed methods
(`$resolveWebviewView`, `$acceptStaticEntries`), registration is
one-live-seat-per-id with a throw on duplicates, and the renderer side of
the bridge ships in the same artifact with matching RPC names. DropTerm's
lua-host already implements the extension-side of this shape; the
typed-slot upgrade is a contract upgrade, not a new architecture.

## Findings

### 1. The seam is one identifier table of typed proxies — the "slots"

The extension-host bundle defines an identifier table mapping string ids
to RPC proxies — `MainThreadStatusBar:q("MainThreadStatusBar")`,
`MainThreadQuickOpen:q("MainThreadQuickOpen")`, … — and every UI-facing
ext-host part holds its counterpart through it:

```js
getProxy(B.MainThreadStatusBar)   // ext-side status bar part
getProxy(B.MainThreadWebviewViews)
getProxy(B.MainThreadTreeViews)
```

The `getProxy(B.MainThread*)` inventory counts **~90 distinct slots**
(commands, dialogs, webviews, webview views, webview panels, tree views,
status bar, editors, terminals, tests, …). Extensions never receive
widgets; they receive ids and proxies. (Byte inspection,
`extensionHostProcess.js`, digest `2a9a4c9a…` below.)

### 2. The status bar slot: typed args in, host renders

The public API parses typed overloads and hands the slot to the ext-host
part:

```js
createStatusBarItem(S,C,D){…return Pt.createStatusBarEntry(P,Le,Ot,Tn)…}
setStatusBarMessage(S,C){return Pt.setStatusBar…
```

The ext part holds `this._proxy=t.getProxy(B.MainThreadStatusBar)` and
also *receives* typed host calls — `$acceptStaticEntries(entryId…)` —
whose name appears on **both** bundles: twice in the extension host, and
twice in the 22 MB renderer bundle beside the renderer-side identifier
table `MainThreadStatusBar:Uo("MainThreadStatusBar")` and a decorated
service wired `w([an(Mo.MainThreadStatusBar)…])`. One contract, two
sides, both shipped.

### 3. The panel slot (webview views): one live seat per id, host-asks-extension

```js
this._proxy=t.getProxy(B.MainThreadWebviewViews)
registerWebviewViewProvider(t,e,n,o){
  if(this._viewProviders.has(e))
    throw new Error(`View provider for '${e}' already registered`);
  this._viewProviders.set(e,{provider:n,extension:t}…)
  …this._proxy.$unregisterWebviewViewProvider(e)…}
async $resolveWebviewView(t,e,n,o,i){…}
```

Three contract facts, first-hand: duplicate registration **throws**
(one live seat per view id — the same registry semantics DropTerm's
lua-host already uses for segments/commands); disposal crosses the bridge
(`$unregisterWebviewViewProvider`); and when the host opens the slot it
calls **into** the extension (`$resolveWebviewView(viewId, …)`) — the
extension fills a host-owned container on the host's schedule, never by
seizing layout.

### 4. The tree slot: validate-and-throw, disposable out

```js
registerTreeDataProvider(e,n,o){let i=this.createTreeView(e,{treeDataProvider:n},o);return{dispose:()=>i.dispose()}}
createTreeView(e,n,o){if(!n||!n.treeDataProvider)throw new Error("Options with tr…
```

Bad shapes throw at registration; good ones return a disposable. The
same fail-loud registration discipline DropTerm's ports already follow.

### 5. REA grounding and its honest limit

`analyze_javascript_application` (REA JavaScript application analyzer,
evidence `ev_cd90a906…`) ingested the whole `resources/app` directory
(sha256 `c219fc07dda12103d778433329e830d29624ec1de7878321a65fd064b865bd63`,
VS Code 1.147.0, fetched 2026-11-24 from
`update.code.visualstudio.com/latest/linux-x64/stable`), selected
`package.json` + `out/vs/workbench/api/node/extensionHostProcess.js`,
and parsed the 2.3 MB ext-host bundle inertly: 602,072 AST nodes
visited, 0 parse failures. The semantic graph then hit its 20,000-node
per-file ceiling on the minified bundle (76,676 unknowns retained), so
**identifier-level facts above come from direct byte inspection
(`grep`) of the exact files REA digested** — cited by sha256, not by
graph entities. File digests: `extensionHostProcess.js`
`2a9a4c9a4805c6a2f6689a6990e4de9f5e37d6c1207a9c381cca2a9214aa60a6`;
`workbench.desktop.main.js`
`cfd5672ad17ff2c31658327065de80c86393e64b0cc13a946313ef8bc823ec8f`.

## What DropTerm has today

lua-host already ships the extension-side of this exact shape: scripts
register into host-owned registries (segments, commands, tab-title
provider), one live seat per id, replacements log loudly, the host
drains and asks on its own schedule, and every dispatch is fire-and-forget
over typed channels. What VS Code has that DropTerm's slots do not yet:
typed *content* answers (the segment slot still takes one plain string),
a dialog slot, and host-owned panel containers. The gap is contract, not
architecture.

## Recommendation

Adopt the typed-slot architecture as ruled (owner, 2026-11-24, "real
VS Code: typed slots"): grow existing slots' answers into closed typed
vocabularies, add a dialog slot, add host-owned panel containers, and
only then a panel slot — in that order, so every new slot lands against
a container that already exists. Rejected: the free-form scripted
toolkit (owner's ruling; also no surveyed peer ships one — see
[lua-declarative-ui-precedents](lua-declarative-ui-precedents.md)) and
the middle "slots + one free-form canvas" variant (declined at the same
ruling). VS Code's own staging is the cited precedent for slot ordering:
status item early, webview views years later, always behind a fixed
shell.

## Sources

- VS Code 1.147.0 stable, linux-x64 tarball, fetched 2026-11-24 from
  `https://update.code.visualstudio.com/latest/linux-x64/stable` — the
  analyzed artifact; directory sha256 `c219fc07…` above.
- `resources/app/out/vs/workbench/api/node/extensionHostProcess.js`
  (sha256 `2a9a4c9a…`) — findings 1–4 ext-side quotes; parsed by REA
  (602,072 AST nodes, 0 failures).
- `resources/app/out/vs/workbench/workbench.desktop.main.js`
  (sha256 `cfd5672a…`) — renderer-side identifier table
  (`MainThreadStatusBar:Uo(…)`) and `$acceptStaticEntries` counterpart.
- REA evidence: `ev_cd90a90601702b3968e343e1c214e02eb8216c26180352143b15f272f854c690`
  (application graph, retention complete-record), summary view
  `ev_9cdc36b7…`; feature traces `ev_dd8ff34f…` / `ev_f760d4e0…`
  (no-match on literal seeds — the recorded reason is finding 5's
  ceiling, not absence).

## Unverified

- The renderer-side class bodies (the actual `MainThreadStatusBar`
  service internals) were not analyzed — the 22 MB bundle exceeds
  REA's per-file graph budget; only its identifier table and RPC-name
  counterparts were read at byte level.
- The RPC wire format (message framing between ext host and renderer)
  was not inspected; the contract-level fact (typed proxies + `$`
  methods) does not depend on it.
- `$resolveWebviewView`'s full payload (state, context, webview handle
  marshaling) was not traced past the registration quote.
