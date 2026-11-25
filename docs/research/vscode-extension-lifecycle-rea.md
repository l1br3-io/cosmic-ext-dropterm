# VS Code's extension lifecycle and capability catalog, read from the shipped 1.147.0 artifact (REA-grounded)

**Question:** How do VS Code extensions *behave* — boot, activation, the
capability slots they call, the manifest they declare — per the shipped
1.147.0 artifact, and which of those capabilities should DropTerm's plugin
capability incorporate next?

**Date:** 2026-11-25

**TL;DR:** The shipped artifact proves the extension lifecycle end to end:
the host is booted by a *pushed* registry delta (`$startExtensionHost`),
activation is event-driven from manifest declarations plus extension-point
generators (25 activation namespaces live across the built-in manifests),
`onStartupFinished` activations defer past a measured 50 ms startup budget,
and a pure-declarative extension (no `main`) never activates code at all.
Every capability an extension gets is one typed `MainThread*` slot — 97
identifiers in this build, including the new `MainThreadDialogs`
request→typed-result slot and `MainThreadQuickOpen`'s
`showQuickPick`/`showInputBox` — with commands as
register/unregister Disposable pairs, one-seat-per-id throws, per-extension
frozen storage contexts, output channels split from loggers, and a
progress slot with a cancellable handle. The incorporable capabilities for
DropTerm, in order: a quick-pick/input kind on the existing dialogs slot,
a declarative manifest with activation events, output channels, a typed
storage slot, then progress — progress only once the host grows async work.

## Findings

### 1. Boot is a pushed delta, never a scan

The renderer hands the host its registry; the host never scans:

```js
async $startExtensionHost(e){e.toAdd.forEach(a=>a.extensionLocation=
b.revive(a.extensionLocation));let{globalRegist…
this._myRegistry.set(o),Hd&&(this._logService.info(`$startExtensionHost:
global extensions: ${Yh(this._globalRegistry)}`…
```

DropTerm's host already scans the `plugins/` dir itself; the *behavioral*
fact worth keeping is the delta shape — extension sets change by delta
(`toAdd`), not by re-scan.

### 2. Activation is event-driven, derived three ways

The manifest's `activationEvents` array is read verbatim
(`Array.isArray(t.activationEvents)?t.activationEvents.slice(0):[]`), and
extension points register *generators* that derive activation events from
declarative contributions:

```js
t.activationEventsGenerator&&Y0.register(t.extensionPoint,
t.activationEventsGenerator)
…extensionPoint:"taskDefinitions",activationEventsGenerator:
function*(r){for(let t of r)t.type&&(yield`onTaskType:${t.type}`)}
…yield`onLanguageModelChatProvider:${t.vendor}`
```

A pure-declarative extension needs no activation at all:

```js
typeof t.main>"u"&&typeof t.browser>"u")return[]
```

Implicit command-derived activation is proven behaviorally: the marketplace
Error Lens declares `activationEvents: ['onStartupFinished']` and nothing
else, yet ships 15+ commands wired into menus and keybindings — the host
must derive `onCommand:` events for contributed commands. The built-in
manifests spell theirs out: 18 of the 19 command-declaring built-ins list
every `onCommand:` id explicitly (e.g. js-debug carries 10 `onCommand:` +
11 `onDebugResolve:` entries in one array).

### 3. Startup-finished activation defers on a measured budget

```js
if(l==="onStartupFinished")if(Date.now()-i>50){this
._activateAllStartupFinishedDeferr…
```

If startup already took more than 50 ms, startup-finished activations
defer. Laziness is not just per-event; it is cost-measured against the
startup the app just paid.

### 4. The slot table: 97 typed `MainThread*` identifiers

The identifier table in the extension-host bundle counts **97** distinct
`MainThread*` strings (prior record: ~90). New since the prior record's
count: the chat family (`MainThreadChatStatus`, `ChatSessions`,
`ChatQuota`, `ChatOutputRenderer`, `ChatInputNotification`, `ChatDebug`,
`ChatContext`, `ChatArtifactIntegrations`, `ChatAgents`, `ChatSkills`),
`MainThreadMcp`, `MainThreadLanguageModelTools`, `MainThreadEmbeddings`,
the `MainThreadAi*` trio, `MainThreadDialogs`, `MainThreadLocalization`,
`MainThreadSpeech`, `MainThreadPower`, `MainThreadMeteredConnection`,
`MainThreadShare`, `MainThreadTheming`, `MainThreadBulkEdits`, and more.
One shipped wart is visible in the table itself:
`MainThreadDialogs:q("MainThreadDiaglogs")` — the proxy key is spelled
`Dialogs`, the mapped string id carries upstream's `Diaglogs` typo.

### 5. The dialogs slot is request → typed result

```js
var r_=class{constructor(t){this._proxy=t.getProxy(B.MainThreadDialogs)}
showOpenDialog(t){return this._proxy.$showOpenDialog(t).then(e=>e?e.map(
n=>b.revive(n))…
```

Not a press-event stream: a typed request whose promise resolves to typed
values (`Uri`s revived on arrival). `showSaveDialog` sits beside it on the
same `window` namespace.

### 6. Quick-input is the same shape with item events

```js
showQuickPick(c,u,m,g=ye.None){this._onDidSelectItem=void 0;
let h=Promise.resolve(u),f=++this…
showQuickPick(S,C,D){return oa.showQuickPick(P,S,C,D)},
showInputBox(S,C){return oa.showInput(S,C)},
showOpenDialog(S){return TA.showOpenDialog(S)}…
```

`showQuickPick` resolves to the picked item (typed), reports selection
events, and instances are counted per call — the input slot DropTerm's
dialogs do not have yet.

### 7. Commands: Disposable register/unregister, validate-and-throw

```js
this._commands.set(e,{callback:n,thisArg:o,metadata:i,extension:s}),
t&&this.#e.$registerCommand(e),new Me(()=>{this._commands.delete(e)&&
t&&this.#e.$unregisterCommand(e)})
…registerApiCommand(t){…if(!s.validate(n[a]))throw new Er…
```

Every registration returns a Disposable that unregisters across the
bridge; API-command arguments are validated per-arg with a throw. One-seat
throws elsewhere: URI providers ("Browser URI provider already registered
for …"), editor serializers ("already registered"), terminals
("Terminal has already been created").

### 8. Storage is per-extension and frozen

```js
globalStorageHome:i&&o?.useDefaultFlags?.globalState?i.globalStorageHome
:Ct(e,"globalStorage")…
return Object.freeze({globalState:i,workspaceState:s,secrets:a,
subscriptions:[],get languageModelAccess…
```

The extension context is a frozen object of per-extension `globalState`,
`workspaceState`, `secrets` — storage addressed by extension identifier,
never a shared namespace.

### 9. Output channels and loggers are separate slots

`createOutputChannel` validates the name and throws
(`"illegal argument \`name\`. must not be f…"`), channels track a
`visibleChannelId` for the visible one; `createLogger` is a distinct
facility with `group`/`hidden` options. Logs are a capability surface, not
one shared namespace.

### 10. Progress is a cancellable handle factory

```js
async withProgress(t,e,n){let o=this._handles++,{title:i,location:s,
cancellable:a,notifi…
```

Each call allocates a handle, carries `{title, location, cancellable}`,
and the reporter lands through a deferred (`this.reporter=o`). The
cancellable flag crosses the bridge; cancellation is the extension-side
promise rejecting.

### 11. File watching is a host slot feeding typed events

```js
createFileSystemWatcher(t,e,n,o,i,s){return new S2(this._mainContext,e,n,
t,o,this._onFileSystemEvent.ev…
createFileSystemWatcher:(S,C,D,Le)=>{let Ot={ignoreCreateEvents:!!C,
ignoreChangeEvents:!!D,ignoreDelete…
```

The extension declares ignore-flags declaratively; the host's event stream
delivers create/change/delete as typed events.

### 12. The declarative vocabulary, counted over 95 built-in manifests

| `contributes` key | Extensions declaring it |
|---|---|
| `languages` | 56 |
| `grammars` | 52 |
| `configuration` | 24 |
| `configurationDefaults` | 23 |
| `commands` | 19 |
| `menus` | 15 |
| `snippets` | 14 |
| `jsonValidation` | 10 |
| `themes` | 10 |
| `keybindings` | 5 |
| … plus 40 more keys down to 1 (incl. `views`, `viewsContainers`, `terminal`, `icons`, `colors`, `chatAgents`, `languageModelTools`, `mcpServerDefinitionProviders`) | |

Activation namespaces across those manifests: 25 distinct prefixes —
`onLanguage:` 26, `onCommand:` 23, `onDebugResolve:` 11,
`onFileSystem:` 5, `onTaskType:` 5, `onStartupFinished` 4, `*` 3,
`onWebviewPanel:` 3, and a long tail (`onUri`, `onTunnel`, `onProfile`,
`onSpeech`, `onWalkthrough:`, `workspaceContains:` — the last a
declarative file-glob predicate the host evaluates, with an early bail on
`*`). The vocabulary is even productized into the editor's own snippet
bodies (`onLanguage:${1:languageId}`, `onCommand:${2:commandId}`).

### 13. The extension side, ingested: Error Lens

`analyze_javascript_application` ingested the extracted marketplace
extension (directory sha256 `26f3ed9b…`): 18,768 AST nodes visited,
0 parse failures, a real data-flow graph retained (7,220 data-flow and 375
call-flow relations). Its manifest: `activationEvents:
['onStartupFinished']`, `contributes: commands, configuration, colors`,
15+ commands — the lazy marketplace shape: declare everything, run code
once, after startup. A literal-seed feature trace through its graph
(`errorlens.toggleWorkspace`) returned `no-match` — the recorded limitation
class of the prior record (minified bundles bound the semantic graph);
not evidence of absence.

## What DropTerm has today

The lua-host ships the extension side of the slot shape (prior record's
grounding) and has since grown the contract gap closed: typed answers,
the dialogs slot (buttons; press/dismiss as `slot-event`), host-owned
panel containers with panel views, status segments, tab titles, script
commands (keybindable, fire-and-forget, `{ tab }` arg), `send_text`, the
event bus with seven terminal events, timeouts, whole-interpreter reload,
full trust. What remains VS Code-shaped and absent: any *input* surface
(dialogs carry buttons, not text or selection), any laziness model (every
script loads on the first work item), named output channels (one shared
`dropterm:` log namespace), per-script storage (scripts use raw `io`),
progress/cancellation (dispatches are synchronous fire-and-forget under
the 100 ms / 1 s bounds).

## Recommendation

Incorporate, in this order, each against a container that already exists
(the typed-slots staging rule):

1. **Quick-pick and input kinds on the dialogs slot.** The dialog slot is
   the live container; VS Code's `MainThreadQuickOpen`/`MainThreadDialogs`
   prove the shape is request → typed result (a picked id, a text value),
   with dismiss events beside. Biggest user-visible gap, smallest new
   machinery.
2. **A declarative plugin manifest with activation events.** A
   `plugin.json` beside each script declaring commands (later segments)
   lets the host advertise them — settings rows, palette entries — without
   spawning an interpreter, and activate the script lazily on first use
   (`on_command:<id>` first; an `on_startup_finished` with a measured
   defer for eager scripts is the VS Code cost-model precedent). This is
   the VS Code-side grounding for the declarative-schema module already
   tracked by [lua-declarative-ui-precedents](lua-declarative-ui-precedents.md).
3. **Named output channels.** `dropterm.output('name')` giving each script
   its own visible log surface (a rail view is the natural DropTerm
   container) instead of the one shared `dropterm:` namespace.
4. **A typed storage slot.** Per-script key-value `globalState` the host
   persists and hands back frozen — survives interpreter teardowns by
   design, unlike script-side files that the reload semantics cannot
   clean up. Lower priority: full-trust scripts already have `io`.
5. **A progress slot, gated.** Only when the host grows asynchronous work;
   DropTerm's dispatches are synchronous fire-and-forget under hard
   bounds today, so there is nothing to cancel yet.

Rejected or gated, with grounds: webviews and any free-form canvas
(ruled out, 2026-11-24); plugin-declared keybindings and menus (DropTerm
ruled scripts never bind chords — the user binds in settings only;
VS Code's declarative `keybindings`/`menus` are the counter-model, kept
out until an owner ruling reverses that); terminal-creating verbs
(`createTerminal` analog) — every verb is a permanent API commitment and
v1 deliberately shipped one (`send_text`); a file-watcher slot (full trust
already grants `io`; a typed watcher is host-side plumbing for later).

## Sources

- VS Code 1.147.0 stable, linux-x64 tarball, fetched 2026-11-25 from
  `https://update.code.visualstudio.com/latest/linux-x64/stable` (same
  build as the prior record; identical file digests).
- `resources/app/out/vs/workbench/api/node/extensionHostProcess.js`
  (sha256 `2a9a4c9a4805c6a2f6689a6990e4de9f5e37d6c1207a9c381cca2a9214aa60a6`)
  — findings 1–11 quotes; ingested by REA
  (`ev_075f95e44e57f42712fc0310e13ff16243f50e8176385361cdd59b176a923615`,
  602,072 AST nodes, 0 parse failures, 20,000-node per-file ceiling with
  the omission census retained: 30,816 dynamic-call and 48,748
  ambiguous-target object-flow occurrences); identifier facts cite
  byte inspection of the same digested file.
- `vsix/extracted/extension` (Error Lens, usernamehw.errorlens, latest
  VSIX fetched 2026-11-25 from the Marketplace gallery; directory sha256
  `26f3ed9babd12df725f7796eb1147298b5b5bd0f0787ab7ab6ad14e0df890d40`) —
  finding 13; REA application graph
  `ev_c16855b74af8acc241f367c3bda79d26f8f81bc9b3e6e848d58c76e84a21b3b7`,
  feature trace `ev_0c78c15795dac20dc37ac1bfd44e69c7b5a0e55efb3f5c322906814ccdd3e6f7`
  (no-match, recorded).
- 95 built-in extension manifests under
  `resources/app/extensions/*/package.json` — findings 2 and 12 (census by
  JSON parse of the fetched artifact).
- Prior grounding: [vscode-typed-slots-rea](vscode-typed-slots-rea.md)
  (2026-11-24) — the slot-table architecture this record extends from
  contract to lifecycle.

## Unverified

- The renderer-side 22 MB workbench bundle was not ingested (same budget
  limit as the prior record); the commands/menus extension-point
  registrations and the implicit `onCommand:` generator live there — the
  implicit-activation claim is proven behaviorally (finding 2's Error Lens
  argument), not by a captured generator.
- The RPC wire format and the `Diaglogs`-typo string's downstream mapping
  were not traced; the request→result contract facts do not depend on them.
- Error Lens was parsed inertly, never executed; its runtime behavior is
  unobserved.
- The 50 ms defer figure is read from the shipped bundle's constant; the
  constant's origin (a tunable or a hardcode) was not chased.
