---
name: desktop-ui-design
description: "Desktop GUI design for native and hybrid applications: platform conventions (macOS HIG, Windows Fluent, GNOME HIG), menus, keyboard shortcuts, window management, session persistence, native vs Electron/Tauri trade-offs, and accessibility APIs. Use for Swift/AppKit, C#/WinUI, GTK+, or Electron/Tauri/Flutter desktop design. Add web-ui-design for a web app shipped as a desktop wrapper. Not for browser-only interfaces, terminal applications (use tui-design), or command-line tools (use cli-design)."
metadata:
  version: "1.0.0"
---

# Desktop UI Design

## What this skill is for

Desktop GUI design is the discipline of designing persistent, windowed applications that run as first-class OS processes. Desktop apps have distinct obligations that do not exist on the web: they persist across interactions without reloading, integrate with the operating system's window manager and services, participate in a platform ecosystem governed by published Human Interface Guidelines, and carry deep user muscle-memory from years of OS use.

**Load this skill when:**
- Designing a native macOS, Windows, or Linux desktop application
- Designing an Electron, Tauri, or Flutter desktop hybrid application
- Designing the menu bar structure, keyboard shortcut vocabulary, or window management model for a desktop app
- Making a design decision that involves OS integration: notifications, clipboard, file system, system tray/Dock
- Specifying application state persistence — what the app remembers between sessions
- Reviewing a desktop app's design for platform convention violations

**Do NOT load this skill when:**
- The interface is a web application in a browser, even a complex one (→ `web-ui-design`)
- The interface is a terminal-based TUI (→ `tui-design`)
- The interface is a command-line tool (→ `cli-design`)
- Load alongside `web-ui-design` only if designing a web app that also wraps in a desktop shell, to address the shell-specific design obligations (menu bar, keyboard shortcuts, window state)

**Boundary with `interaction-design`:** `interaction-design` owns the generic behavioral contract — the eight canonical states, feedback taxonomy, error recovery — that applies across all surfaces. This skill owns how those patterns manifest within the OS-hosted windowed context: how modality works with OS window management, how error dialogs relate to application state, what loading states look like when the OS provides its own progress indicators.

**Boundary with `accessibility-design`:** `accessibility-design` owns disability-type analysis and WCAG/ARIA standards. This skill owns how desktop accessibility APIs (macOS Accessibility API, Windows UI Automation, AT-SPI/ATK) differ from web ARIA, and what design decisions create or destroy accessible desktop applications.

---

## Platform Conventions as Design Constraints

Desktop users have spent years learning their platform's vocabulary of interactions. That vocabulary is not arbitrary — it is documented in authoritative Human Interface Guidelines maintained by each platform vendor. Deviating from platform conventions without a justified reason is a form of user betrayal: users press the wrong key, reach for the wrong menu, or fail to find an action because they brought correct prior knowledge to an application that violated it.

### The three authoritative HIGs

Every platform convention design decision should be checked against:

| Platform | HIG | URL |
|---|---|---|
| macOS | Apple Human Interface Guidelines | developer.apple.com/design/human-interface-guidelines/ |
| Windows | Microsoft Fluent 2 Design System | fluent2.microsoft.design |
| GNOME Linux | GNOME Human Interface Guidelines | developer.gnome.org/hig/ |

These are the primary constraint documents. They define what "correct" means on each platform. When you deviate from them, you owe the user a justified reason, and the Design Spec must name the deviation and its justification.

### macOS conventions (Apple HIG)

- **Primary modifier:** `Cmd` (⌘) — not Ctrl. macOS users have bone-deep muscle memory for Cmd+C, Cmd+V, Cmd+Z. Using Ctrl for application shortcuts on macOS creates systematic errors — the user presses Ctrl+S and triggers a terminal-style interrupt instead of saving.
- **Global menu bar:** The menu bar at the top of the screen belongs to the currently focused application. It is not inside the application window. Standard menu names: **[App Name], File, Edit, View, Window, Help**. Deviating from this order or omitting standard menus signals unfamiliarity with the platform.
- **Window controls:** Red/yellow/green traffic light buttons in the top-left corner of every window — always. Red = close, yellow = minimize, green = zoom/full screen. Replacing or relocating these breaks the user's spatial memory for window management.
- **Dock presence:** Apps appear in the Dock while running. A dot indicates an open app. Clicking the Dock icon brings the app forward if running, or launches it if not. The Dock is how macOS users manage their running app landscape — a desktop app must participate in it.
- **Document tabs:** macOS supports native per-window tab management via the Window menu. Apps that host multiple documents should consider tab support.
- **System services:** spell check, Handoff, Share sheet, Spotlight integration — these should work in text fields and app surfaces unless there is a specific reason to disable them.
- **Reserved shortcuts (apps must NOT intercept):** Cmd+Space (Spotlight), Cmd+Tab (App Switcher), Cmd+Shift+3/4/5 (Screenshots), Cmd+H (Hide), Cmd+M (Minimize), Cmd+Option+M (Minimize All), Cmd+` (Cycle Windows), Cmd+Q (Quit — this is standard; apps should handle it gracefully but not intercept it to mean something else).

### Windows conventions (Fluent 2 / Windows HIG)

- **Primary modifier:** `Ctrl` — not Cmd. Ctrl+C, Ctrl+V, Ctrl+Z are the universal Windows vocabulary.
- **Window chrome:** The title bar is part of the window, not a system-wide resource. Windows apps may use Fluent-style acrylic materials in the title bar region. The system provides the window control buttons (minimize, maximize/restore, close) in the top-right corner.
- **Alt+F4:** Closes the application. Apps must not intercept Alt+F4 to do something other than close. It is the user's last resort when an application is unresponsive.
- **Application menu bar:** May be embedded in the window (traditional), implemented as a Ribbon (Office-style), or omitted in favor of hamburger menus (more common on Windows than macOS for simpler apps). The design must choose consciously.
- **System tray (Notification Area):** A legitimate integration point for background applications — backup software, sync clients, messaging. Design the tray icon, its tooltip, its context menu, and what happens when the user double-clicks it.
- **Right-click context menus:** Users expect context menus with standard items: Cut, Copy, Paste, Select All, and app-specific items relevant to the selected element. Non-standard context menus (or no context menu) break user expectations.
- **Jump Lists:** Windows taskbar supports Jump Lists — recent files and common actions pinned to the taskbar icon. Design what belongs in the Jump List for document-centric applications.
- **Reserved shortcuts (apps must NOT use):** Ctrl+Alt+Delete (system interrupt), Alt+Tab (task switcher), Windows key combinations (Windows+L, Windows+D, etc. are OS-reserved), Alt+F4 (close application).

### GNOME Linux conventions (GNOME HIG)

- **Primary modifier:** `Ctrl` for most actions, `Ctrl+Shift` for reverse/extend (e.g., Ctrl+Z undo, Ctrl+Shift+Z redo). `Alt` for access keys (underlined letters in menus). **Never use** the Super (Windows) key — it is system-reserved.
- **Header Bar (CSD):** GNOME applications use client-side decorations — the Header Bar is the primary window chrome and typically contains: a back button (when navigating), the window title or primary search, and a menu button (hamburger, F10 to open). Traditional title bar + menu bar is not idiomatic GNOME.
- **Application menu:** Typically accessed from the Header Bar's hamburger menu or from F10. The application menu contains: about, preferences, keyboard shortcuts, quit. Not a separate global menu bar.
- **Standard shortcuts:** F1 (help), F9 (toggle side pane), F10 (primary menu), Ctrl+? (keyboard shortcuts dialog). These are conventions the GNOME HIG establishes for all GNOME apps — implement them.
- **Keyboard navigation requirement:** "Every action should be possible with the keyboard." GNOME HIG treats complete keyboard navigability as a first-class design obligation, not an accessibility afterthought. Tab, arrow keys, Enter, Escape must navigate all UI elements.
- **Flat design aesthetic:** GNOME Libadwaita components follow a flat design system. Avoid skeuomorphic or platform-alien visual approaches.

### Cross-platform keyboard conventions

Actions that have converged across platforms (different modifier, same meaning):

| Action | macOS | Windows / GNOME |
|---|---|---|
| Undo | Cmd+Z | Ctrl+Z |
| Redo | Cmd+Shift+Z | Ctrl+Shift+Z |
| Cut | Cmd+X | Ctrl+X |
| Copy | Cmd+C | Ctrl+C |
| Paste | Cmd+V | Ctrl+V |
| Select All | Cmd+A | Ctrl+A |
| Find | Cmd+F | Ctrl+F |
| Save | Cmd+S | Ctrl+S |
| New | Cmd+N | Ctrl+N |
| Open | Cmd+O | Ctrl+O |
| Close window | Cmd+W | Ctrl+W |
| Quit app | Cmd+Q | Ctrl+Q (Ctrl+F4 in some Windows contexts) |
| Print | Cmd+P | Ctrl+P |

These shortcuts are expected to work correctly in every desktop application. Repurposing any of them for an unrelated action is a major usability defect.

### When to deviate from platform conventions

Deviation is a design decision that must be justified. Cost of deviation: the user's existing muscle memory produces errors, their confidence in the application decreases, and discoverability degrades. Justified reasons to deviate:

- **Cross-platform consistency:** a hybrid app that ships identically on macOS and Windows may converge on a single shortcut map rather than adapting per platform. The cost (worse native feel on each platform) must be weighed against the benefit (single codebase, single test matrix). This must be a deliberate choice in the Design Spec.
- **Domain-specific vocabulary:** an application for a professional domain (music production, 3D modeling) may have a well-established shortcut vocabulary from industry-standard tools. Users who migrate from Pro Tools or Maya carry those shortcuts. Matching industry-standard shortcuts may outweigh matching platform conventions.
- **Direct conflict:** when two platform conventions conflict within the same application context. Document the conflict and the resolution.

---

## Menu Bar Design — The Discoverability Mechanism

The menu bar is not decoration. It is the primary discoverability mechanism for desktop applications — the complete catalog of everything the application can do, organized by category. A user who has never used your application can open the File menu and learn what file operations are available, then open the Edit menu and discover editing capabilities they didn't know existed. This discoverability has no equivalent in web or mobile design.

### Why the menu bar matters

1. **Commands are listed with keyboard shortcuts.** A user discovers "Save" in the File menu and sees "Cmd+S" next to it. They learn the shortcut while using the menu. This is the primary keyboard shortcut discovery path — not documentation, not tooltips, the menu bar.
2. **Disabled menu items communicate context.** When "Paste" is grayed out in the Edit menu, the user knows the clipboard is empty or the selection is in a non-editable area. When "Save" is grayed out, the document is already saved. Disabled menu items are a form of feedback.
3. **Menu organization reveals structure.** The presence of a "Layers" menu tells the user this application has a layers concept. The presence of "View > Zoom" shows the application can zoom. Menu structure is information architecture in application form.

### Standard menu structure (macOS HIG pattern — adapt by platform)

On macOS, the global menu bar standard order is:

- **[App Name] menu** — About [App Name]; Preferences/Settings; Services; Hide [App Name]; Hide Others; Show All; Quit [App Name]
- **File** — New; Open; Open Recent; Close; Save; Save As; Revert to Saved; Export; Share; Print
- **Edit** — Undo; Redo; Cut; Copy; Paste; Paste and Match Style; Select All; Find/Replace; Spelling and Grammar; Substitutions; Transformations
- **View** — toolbar visibility; sidebar visibility; zoom; full screen
- **Window** — Minimize; Zoom; Tile Window; Move to Display; Bring All to Front; [list of open windows]
- **Help** — Search (Spotlight-style command search); [documentation links]

**App-specific menus** go between View and Window. A word processor adds Format. An IDE adds Run, Debug. A graphics tool adds Image, Layer, Filter. Keep the standard menus in their standard positions.

On Windows, the menu bar is embedded in the window (when present) and may use a ribbon instead for complex applications. On GNOME, the application menu is typically in the Header Bar's menu button.

### Menu design rules

**Every frequently-used command must have a keyboard shortcut.** If it is in the menu, it should have a shortcut. Menus with no keyboard shortcuts listed are discoverability failures — the user sees the command exists but cannot learn how to access it efficiently.

**Destructive actions go at the bottom of their menu, away from frequently-used commands.** "Delete" should not be adjacent to "Duplicate." Placing a destructive action next to a frequently-used safe action invites accidental triggering.

**Ellipsis (…) signals "more information required."** A menu item ending with "…" will open a dialog before doing anything. "Export…" means the user will be asked where and in what format. "Print…" means a print dialog will appear. A menu item without "…" executes immediately: "Undo" undos now, "Quit" quits now. This convention sets user expectations about what will happen next.

**Submenus should be used sparingly.** A cascading submenu imposes motor cost — the user must move the mouse into the submenu's zone without it collapsing. Submenus are harder to keyboard-navigate. Use them only when the top-level menu is already appropriately organized and a category truly needs subdivision.

**Menu items should be consistent in form.** Within a menu, labels should be parallel: either all nouns, or all verbs, or all verb-object phrases. "Cut", "Copy", "Paste", "Find…" — all actions. Do not mix "Find…" with "Content Search" and "Show Replacements Panel."

### Contextual menus (right-click)

Contextual menus should contain commands that are:
- Relevant to the currently selected or pointed-at element
- Commonly used enough to merit shortcut access
- Either too contextual for the main menu bar, or repeated from it for convenience

Contextual menus are **not** a replacement for the menu bar. An action that exists only in a contextual menu is an action that power users cannot discover through the menu bar and cannot access via keyboard shortcut without right-clicking first. All significant actions must be in the menu bar.

### Toolbar

The toolbar is the collection of frequently-used commands that deserve persistent visual prominence. Toolbar membership criteria:
- The command is used frequently enough that users benefit from a one-click shortcut
- The command benefits from a visual icon that helps users recognize it without reading text
- The command's icon is recognizable in the toolbar context (not just any action warrants an icon)

Not everything in the menu bar belongs in the toolbar. Not everything in the toolbar must be unique to the toolbar — toolbar buttons often duplicate menu items with an icon-shortcut affordance.

---

## Keyboard Shortcut Design

Keyboard shortcut design for desktop applications is a discipline. A poorly designed shortcut map produces user errors, frustration among power users, and inaccessible applications for users who depend on keyboard navigation.

### Platform modifier conventions

- **macOS:** `Cmd` (⌘) is the primary modifier for application shortcuts. `Cmd+Option` for secondary application shortcuts. `Cmd+Shift` for reverse or extended operations (Undo is Cmd+Z; Redo is Cmd+Shift+Z). `Ctrl` is rarely used for application shortcuts — it is associated with terminal-style controls on macOS and should be avoided for regular application actions.
- **Windows:** `Ctrl` is the primary modifier. `Ctrl+Alt` for tertiary actions. `Alt` for access keys (the underlined letter in a menu item, accessed by pressing Alt to activate the menu bar then the underlined letter). Access keys are a Windows-specific keyboard navigation pattern.
- **GNOME Linux:** `Ctrl` is primary. `Ctrl+Shift` for reverse operations. `Alt` for access keys (same as Windows). Never use `Super` (Windows key) — it is system-reserved.

### Shortcut design rules

1. **Standard shortcuts must do standard things.** Cmd+S (macOS) / Ctrl+S (Windows/Linux) MUST be Save. Cmd+Z / Ctrl+Z MUST be Undo. Repurposing any standard shortcut to mean something else is a major usability defect. Users have no way to anticipate it; they will make errors.

2. **Avoid OS-reserved shortcuts.** Check the platform HIG's reserved shortcut list before assigning any shortcut. macOS reserves Cmd+Space, Cmd+Tab, Cmd+Shift+3/4/5, Cmd+H, Cmd+M, Cmd+`. GNOME reserves Super+anything. Windows reserves Alt+Tab, Ctrl+Alt+Delete, Windows key combinations.

3. **Two-key maximum for primary actions.** Primary actions should be reachable with two keys (modifier + key). Three-key chords (Cmd+Shift+Option+P) are ergonomically poor and hard to memorize for primary actions. Three-key chords are acceptable for secondary or developer-facing actions.

4. **Make shortcuts mnemonic.** Cmd+B for **B**old. Cmd+F for **F**ind. Cmd+I for **I**talic. Mnemonic shortcuts are remembered without a reference table. Non-mnemonic shortcuts require deliberate memorization.

5. **Group shortcuts by modifier.** All navigation shortcuts might share Cmd (macOS) or Ctrl (Windows). Adding Shift extends or reverses. Adding Option/Alt accesses alternate forms. A coherent modifier grammar helps users generalize: "Shift extends the operation, Option accesses the alt form."

6. **One-handed accessibility.** Users with motor impairments may use only one hand. Critical primary actions should have shortcuts achievable one-handed: left-hand-accessible Ctrl+S, Ctrl+C, Ctrl+V are all reachable with one hand on a standard keyboard. Right-hand-only chord combinations may be inaccessible.

7. **Document shortcuts in the menu bar.** The menu bar is the primary shortcut discovery mechanism. Every shortcut must appear next to its menu item. A keyboard shortcut not in the menu bar is a shortcut users cannot discover.

8. **Provide a keyboard shortcuts dialog.** The GNOME HIG specifies Ctrl+? to open a keyboard shortcuts dialog listing all application shortcuts. VS Code popularized the Command Palette (Cmd/Ctrl+Shift+P) as a searchable alternative. Both patterns make the full shortcut map discoverable at once.

### Keyboard shortcuts in hybrid apps

Electron apps run inside a Chromium window. The webview intercepts keyboard events. If your web content uses Cmd+Left for navigation, it conflicts with native macOS behavior (home/beginning of line). Design must explicitly specify which shortcuts are handled at the native (app menu) level and which at the webview (web content) level.

Tauri apps use the system webview (WKWebView on macOS, WebView2 on Windows). Same conflict potential applies. The system webview is closer to native behavior than Electron's Chromium, but conflicts still require explicit resolution.

Design artifact: the keyboard shortcut map is a deliverable, not an implementation detail. It lists every significant shortcut, its modifier, its key, its category, and verifies it against the platform HIG's reserved list.

---

## Window Management as Interaction Design

Window management is not just a display concern — it defines the interaction model for how users work with the application and its content.

### Window models — choose deliberately

**Single-window model:** All content lives within one primary window. The user never manages multiple windows of the same application. Navigation is through sidebars, tabs, panels, and in-window views.

- Examples: VS Code, Figma desktop, Xcode, most mobile-first hybrid apps
- Design implications: tab management within the window, panel visibility toggles, full-screen mode, window restoration to the last layout on relaunch
- When to choose: when the application's core interaction is continuous (IDE, creative tool, productivity suite) and multi-window comparison is not a primary use case

**Multi-window model:** Each document or workspace gets its own window. The OS window manager is the container.

- Examples: Traditional text editors (each file is a window), Preview (each image is a window), image viewers
- Design implications: the Window menu lists all open windows and allows switching; Cmd+W closes the current document window, not the application; closing the last window may or may not quit the app (macOS convention: apps continue running in the Dock; Windows convention: closing the last window exits the app)
- When to choose: when users frequently compare two documents side-by-side, or when the document-centric model is a strong conceptual fit

**Panel architecture:** A primary window with floating or dockable panels for secondary functionality.

- Examples: Photoshop (tool panels, layer panel, property inspector), Logic Pro, most creative professional tools
- Design implications: panels have their own toggle keyboard shortcuts (F9 convention in GNOME; custom shortcuts on macOS/Windows); panels must remember their position and docked/floating state; panels that obscure primary content need a keyboard shortcut to toggle quickly
- When to choose: when secondary information (properties, navigation, tools) is needed frequently but not always, and screen real estate is a significant concern

### Modal vs non-modal dialogs

**Modal dialogs** block the application until dismissed. Use for:
- Destructive or irreversible actions that require explicit confirmation ("Delete 847 files?" "Override save?")
- Information critical to completing the current operation (authentication required, permission denied)
- Focused configuration that must complete before work continues (print settings, export format)

**Non-modal dialogs** (floating windows, panels) allow continued work while open. Use for:
- Tool configuration that adjusts an ongoing activity (Find and Replace, Inspector panel)
- Reference information that supplements current work (a character map, a color picker)
- Secondary workflows that are additive, not blocking

**System-modal dialogs** (dialogs that block all OS windows, not just the application) are almost never justified in modern applications. They prevent the user from consulting a reference in another application while responding to the dialog. Legacy Windows MessageBox calls could create system-modal dialogs; this pattern should be avoided entirely.

**Application sheets (macOS):** On macOS, dialogs for document-specific actions (save, print, export) should use sheets — dialogs that slide down from the window's title bar and are visually attached to their parent window. This communicates that the dialog belongs to this document and does not interrupt work in other windows of the same application.

### Alert and dialog content design

A dialog that requires a decision must give the user what they need to make the decision:
- **Title:** states the situation or decision, not a question ("Unsaved Changes" not "Are you sure?")
- **Body:** provides the specific consequence the user is deciding about ("Your changes will be permanently lost" not "This action cannot be undone")
- **Buttons:** named for what they do ("Delete", "Save", "Don't Save") not generic ("OK", "Cancel") — "OK" communicates nothing about what will happen
- **Default button:** the safe action (not the destructive one) is the default, highlighted button — this is the button that activates when the user presses Enter or Return

### OS window management integration

The application's windows must participate correctly in OS window management:
- **macOS Mission Control / Exposé:** All application windows appear. Windows must be individually identifiable by their title.
- **macOS Stage Manager:** Application groupings must work sensibly.
- **Windows Aero Snap / Snap Layouts:** Windows must be resizable to arbitrary dimensions; content must reflow appropriately. Do not use custom window chrome that prevents Aero Snap activation.
- **GNOME Activities Overview:** All application windows appear. GNOME CSD windows participate natively.

Custom window chrome (common in Electron apps) must not remove these OS integration behaviors. A custom title bar is acceptable; a custom title bar that prevents Aero Snap, does not appear in Mission Control, or cannot be maximized is an OS integration failure.

### Drag-and-drop design

Drag-and-drop is a natural affordance in windowed desktop environments — it is what users do instinctively to move and share content.

**Always provide a keyboard alternative.** Drag-and-drop is inaccessible to keyboard-only users and switch-access users unless an explicit keyboard alternative exists. The alternative must be specified: Cut/Copy + Paste, a "Move to…" context menu item, or dedicated Move Up/Move Down keyboard commands.

**Drop zone affordance.** Drop zones must become visibly active when the user starts dragging — not only when the cursor is over them. Users need to see where they can drop before they have committed to dragging. When the cursor enters a valid drop zone, the zone must provide entry feedback (border color change, label change, highlight). Invalid drop zones must be visually marked as invalid.

**Cross-application drag.** Design for files being dragged from Finder/Explorer into the application window. What file types does the application accept? What happens for unsupported types? What happens for multiple files dropped at once?

---

## Application State Persistence

Desktop users have a deep expectation: when I reopen an application, it will look like I left it. This expectation is not optional — failing to restore state forces users to re-navigate to their work every session.

### State categories

**Ephemeral state** — survives only the current session; intentionally not restored:
- In-progress transient operations (a drag that was interrupted)
- Temporary selection or highlight state
- Partial text entry in a search field (depends on the application — for a long research query, restoring may be appropriate)

**Session state** — should be restored on relaunch:
- Window position and size
- Which document or workspace was open
- Which panels were visible or hidden and at what dimensions
- Scroll position within the primary document
- Current selection or expanded items in tree views
- Active tab within a multi-tab window

**User preference state** — user-configured, persists indefinitely, can be explicitly reset:
- Application settings (theme, language, editor preferences, keyboard shortcut customizations)
- User-defined workspace layouts
- Frequently-accessed locations in file dialogs

**Document state** — belongs to the document, not the application:
- Cursor position within a document
- Zoom level of a specific document
- Collapsed/expanded regions within a document

### Platform persistence mechanisms

| Platform | Preferences | Session state |
|---|---|---|
| macOS | `NSUserDefaults`; Info.plist; App Sandbox (~Library/Application Support) | macOS Resume API (`NSApplicationDelegate restorationClass`); automatic document restoration for AppKit/SwiftUI |
| Windows | Windows Registry; `%APPDATA%` config files | No platform-standard session restoration; app must implement manually |
| GNOME Linux | `gsettings` schemas | `~/.config/<appname>/` directory; app must implement manually |

**For Electron/Tauri apps:** Both frameworks give developers full responsibility for persistence. Key options:
- Electron: `electron-store` for cross-platform key-value storage; `electron-window-state` for window position/size persistence
- Tauri: `tauri-plugin-store` for settings; `tauri-plugin-window-state` for automatic window state restoration

### First-launch design

The first launch of an application has no saved state. The design must specify:
- What the application displays on first launch (empty state, onboarding, default document)
- What defaults are applied to preferences and settings
- Whether onboarding runs on first launch only, or every time until completed
- What state is synthesized from system context (system dark mode, user language, accessibility preferences)

### Autosave and crash recovery

**Autosave** replaces manual save as the user's safety net for document-centric applications. Design decisions:
- How frequently does autosave run? (30 seconds is a common value for text editors)
- Does autosave replace the user's explicit "saved version" or maintain a separate autosave? (macOS document model maintains both; autosave is distinct from an explicit Save)
- How does the user discard autosaved changes and return to the last explicitly saved version? ("Revert to Saved" or "Revert to Last Opened")

**Crash recovery:** The application must detect on next launch that the previous session ended unexpectedly. Design what the recovery experience looks like: a notification ("Unsaved changes from your previous session have been recovered"), an offer to restore or discard, and where the recovered state is shown.

---

## Native vs Hybrid Trade-offs

The design discipline is the same whether the application is native Swift, native C#, Electron, or Tauri — the questions about menu bar structure, keyboard shortcuts, and window management apply equally. Where native and hybrid diverge is in how much platform integration is automatic vs must be explicitly designed and built.

### What native provides automatically

| Concern | Native (AppKit/SwiftUI, WinUI, GTK4) | Hybrid (Electron, Tauri) |
|---|---|---|
| **System fonts** | Platform font rendering: San Francisco (macOS), Segoe UI Variable (Windows), system default (GNOME) | Must configure system font stack in CSS; defaults to Chromium fallbacks |
| **Native scrollbars** | Platform scrollbars: macOS overlay scrollbars with inertia; Windows-style scrollbars | Chromium/WebView scrollbars — differ in appearance and behavior from native |
| **Platform accessibility APIs** | Full native accessibility tree: VoiceOver reads AppKit/SwiftUI controls automatically; Narrator reads WinUI; Orca reads GTK4 | Accessibility through Chromium's ARIA bridge (Electron) or system webview bridge (Tauri); generally good, edge cases exist |
| **Keyboard shortcut integration** | System-level keyboard event handling; standard shortcuts work without explicit implementation | Must explicitly implement shortcuts; webview can intercept events; conflicts require resolution |
| **Native file dialogs** | Platform file dialog: best accessibility, full Finder/Explorer integration, bookmark support | Can use native file dialogs via OS API integration; custom file dialogs are worse in every dimension |
| **Native form controls** | Date pickers, color pickers, number inputs match platform expectations | HTML `<input type="date">` differs from native macOS date picker visually and behaviorally |
| **Drag-and-drop from OS** | System drag-and-drop integration is native | Must be explicitly implemented; generally works through Chromium's integration |
| **Dark mode / accent color** | Automatic: apps respond to system appearance changes without configuration | Requires explicit CSS media queries or system appearance API integration |

### What hybrid gives up

Design implications of choosing Electron or Tauri:

1. **Scrollbar appearance:** macOS's overlay scrollbars disappear when not scrolling and reappear on touch input. Chromium's scrollbar behavior differs. This is one of the most visible "this doesn't feel native" signals in Electron apps. Tauri uses the system WebView (WKWebView on macOS), which handles scrollbars more natively.

2. **Text rendering:** macOS uses subpixel antialiasing for text; Chromium renders text differently. Text in Electron apps on macOS looks slightly off compared to native apps. Not fixable purely through design — must be accepted as a trade-off.

3. **Native menu bar on Linux:** Electron does not support GNOME's global menu bar (application menu in the Shell top bar). Linux users see an in-window menu bar instead of the GNOME-idiomatic application menu in the Header Bar.

4. **Accessibility edge cases:** Electron apps must call `app.setAccessibilitySupportEnabled(true)` or the accessibility tree is not populated (per Electron docs). Custom-drawn controls in the webview do not automatically expose to platform accessibility APIs — they require explicit ARIA annotation.

5. **App bundle size:** Electron ships Chromium (~80-100MB minimum bundle size). Tauri uses the system webview (1-10MB bundle, webview provided by the OS). If bundle size is a design concern for distribution, Tauri is substantially better.

### Hybrid app design obligations

When designing a hybrid application, these integration points must be explicitly designed and specified — they do not come for free:

- **Menu bar structure:** must be explicitly built using the framework's menu API (Electron's `Menu.setApplicationMenu()`, Tauri's `Menu` API). Omitting it is omitting the application's primary discoverability mechanism.
- **Keyboard shortcuts:** must be explicitly registered at the native level for shortcuts that should work when the webview is focused. Shortcuts handled at the web content level are handled by the browser's event model — which can conflict with native OS shortcuts.
- **System fonts:** the CSS font stack must specify platform system fonts: `-apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif` as a minimum. Without this, Electron apps display in Chromium's default font, which looks out-of-place.
- **Dark mode:** the application must listen to system appearance changes and apply appropriate styles. Using `prefers-color-scheme` CSS media query handles this automatically for CSS-based theming.
- **Window state:** must be explicitly persisted using framework-provided storage (electron-store, tauri-plugin-store) or equivalent. No platform-native session restoration applies.
- **System notifications:** must be implemented using the OS notification API through the framework. Electron uses Node.js `Notification` class; Tauri uses the notification plugin.
- **Native file dialogs:** always prefer the native file dialog API over a custom file picker. The native dialog has better accessibility, better Finder/Explorer integration, and matches user expectations.

### Real-world hybrid examples

**VS Code (Electron):** The benchmark for a well-designed Electron app. Custom title bar that closely matches platform conventions per OS. Platform-specific keyboard shortcut handling. Command Palette (Cmd/Ctrl+Shift+P) as a searchable alternative to the menu bar. Native file dialogs. Native system notifications. The accessibility story is strong. Bundle size is large (~200MB), accepted as a trade-off for the cross-platform development experience.

**Notion (Electron):** A weaker example. Known for inconsistent keyboard shortcut support, accessibility gaps, and non-native scrollbar behavior. The design investment in platform conventions is lower; users familiar with macOS conventions encounter repeated friction.

**1Password 8 (Rust + system webview, similar to Tauri):** Deliberately matches platform conventions despite not being native. Uses system fonts, matches platform dark mode, implements native keyboard shortcuts, uses native dialogs. Shows that the platform-convention investment in a webview-based desktop app is a design choice, not a technical constraint.

---

## Desktop Accessibility APIs

Desktop accessibility differs architecturally from web ARIA. The design obligations are similar in goal — every control must expose its role, label, value, and interactions to assistive technology — but the mechanism is different on each platform.

### macOS: Accessibility API (AXUIElement)

- Native AppKit and SwiftUI controls expose their accessibility automatically. VoiceOver reads the accessibility tree without developer intervention for standard controls.
- Custom views must implement `NSAccessibility` protocol: `accessibilityRole`, `accessibilityLabel`, `accessibilityValue`, `accessibilityActions`, `accessibilityHelp`. These are design decisions — what is this control? what is its label? what can it do?
- **Electron on macOS:** accessibility works through Chromium's ARIA-to-AX bridge. The app must call `app.setAccessibilitySupportEnabled(true)`. ARIA roles in the HTML map to macOS accessibility roles. Custom-rendered controls without ARIA are invisible to VoiceOver.
- **Tauri on macOS:** uses WKWebView, which has its own accessibility bridge to VoiceOver. More native-feeling than Electron on macOS.

### Windows: UI Automation (UIA)

- WinUI and WPF expose accessibility through UI Automation automatically for standard controls. NVDA, JAWS, and Narrator all use UIA.
- Custom controls must implement UIA control patterns programmatically — the same design vocabulary (role, name, state, supported actions) expressed through the UIA COM interface.
- **Electron/Tauri on Windows:** both use Chromium-based webviews (Electron: bundled Chromium; Tauri: WebView2), which map ARIA to UIA patterns. Reasonable coverage; complex custom interactions may have gaps that require explicit ARIA annotation.

### Linux: ATK/AT-SPI

- GTK4/Libadwaita applications expose accessibility through ATK (Accessibility Toolkit) and AT-SPI (Assistive Technology Service Provider Interface). Orca screen reader uses AT-SPI.
- **Electron on Linux:** Chromium maps to AT-SPI, with known limitations. Some screen reader interactions that work natively in GTK apps may not work in Electron apps.
- GNOME HIG's keyboard navigation requirement ("every action possible with the keyboard") is the primary accessibility obligation that does not require accessibility API knowledge — keyboard navigability is verifiable through interaction.

### Design implications for accessibility

1. **Every custom-drawn control must declare itself.** A custom-drawn button, slider, or list item does not automatically expose to accessibility APIs. The Design Spec must specify: what role does this control declare? What is its label? What are its accessible actions? Do not leave this to the implementer's judgment.

2. **Keyboard navigation must be complete.** Tab moves focus between interactive elements. Arrow keys navigate within complex widgets (menus, lists, trees, grids). Enter activates the focused element. Escape closes dialogs and dismisses overlays and returns focus to the trigger. This is the minimum; the Design Spec must name the key behavior for every custom interactive component.

3. **Focus visibility is non-negotiable.** The currently focused element must be visually distinguishable. On macOS and Windows, system-provided focus rings are available for native controls. Custom focus indicators must meet WCAG 2.4.11 (minimum 3:1 contrast ratio against adjacent colors; area at least as large as the outline of the unfocused control).

4. **For hybrid apps, ARIA applies.** Electron and Tauri apps expose accessibility through their webview's ARIA bridge. All ARIA authoring practices from `accessibility-design` apply. Web ARIA in the webview maps to the platform accessibility API. Custom React or web components without ARIA roles are invisible to platform screen readers.

---

## System Integration as UX

Desktop apps participate in the OS ecosystem. Each integration point is a design decision.

### Clipboard

Users expect Cmd+C / Ctrl+C to copy selected content in any application. Design must specify:
- What is "selected content" for this application? Text? File paths? Images? A structured data object (e.g., a calendar event)?
- What format is the content placed on the clipboard? For cross-application interoperability, prefer standard formats (plain text, rich text, PNG) over proprietary formats.
- When the user pastes into this application, what clipboard formats are accepted? What is rejected and why?

### System notifications

Desktop apps can send system notifications (macOS Notification Center, Windows Action Center, GNOME notification system).

Design decisions:
- When should a notification be sent? Notification-worthy events: completion of a long background task, receipt of a message addressed to the user, a system status change that requires user action. Not notification-worthy: routine activity, marketing, events the user can see if they look at the app.
- What does the notification contain? Title, body, optionally an action button. Notifications with action buttons (macOS: up to two actions; Windows: up to five) allow the user to respond without opening the app.
- What happens when the notification is clicked? The app should come to the foreground and navigate to the relevant content. A notification that opens the app to its home screen without context is a poor experience.

**Notification anti-pattern:** sending notifications for non-urgent activity trains users to dismiss notifications from the app without reading them. Once users habitually dismiss, even important notifications are ignored.

### File system integration

Desktop apps have direct file system access — unlike web apps that need explicit file picker affordances.

- **Default save location:** where does the app save files by default? Users expect their documents in ~/Documents or a platform-conventional location. Apps that save in unexpected locations break the user's file system mental model.
- **File type associations:** register the application as a handler for relevant file types so users can double-click files to open them. Design what happens when the user opens a file by double-clicking: does it open in the existing app window, a new window, or replace the current document?
- **macOS App Sandbox:** sandboxed macOS apps have restricted file system access. Design must account for where sandboxed apps can save files (~/Documents, ~/Desktop, user-selected locations).

---

## Anti-Patterns (Desktop-Specific)

These patterns appear in desktop applications and represent design failures specific to this surface.

### Splash screens

The user launched the application. They want the application. A splash screen delays this to display branding. Apple HIG explicitly discourages splash screens — "Launch quickly." Show the application window immediately, even if content is still loading. Use skeleton loaders within the window, not a blocking splash screen.

### Installer UX theater

Windows installers that require administrator rights for user-space applications (when no system-level access is needed), present EULA agreements that function as friction, and require system restarts for applications that don't need them — these add cost with no user benefit. Prefer direct distribution: drag-to-Applications on macOS, Windows Store or WinGet, Flatpak or AppImage on Linux.

### Custom window chrome that breaks OS integration

Electron apps commonly implement custom title bars for cross-platform visual consistency. Custom window chrome that removes or repositions OS window controls, prevents Aero Snap, prevents Mission Control grouping, or makes windows unresizable is an OS integration failure. Custom chrome must preserve all OS window management behaviors.

### Non-native file dialogs

Custom file pickers built in HTML/CSS/JS are worse than native file dialogs on every dimension: accessibility is worse, Favorites/Recent Files/network locations are absent, the user's mental model of the file system is not available. Always use the native file dialog API. The rare exception: an application where the file-picking interaction is itself the primary product (a file manager).

### OS-blocking modals

Dialogs that block all OS windows — not just the application — prevent the user from consulting information in another application. This is almost never justified in modern application design. All modal dialogs should be application-modal, not system-modal.

### Overriding standard keyboard shortcuts

Using Cmd+C or Ctrl+C to do something other than Copy is a major usability defect. Using Cmd+W to do something other than Close Window is equally disorienting. The user has built up motor patterns that execute faster than conscious thought — overriding standard shortcuts triggers those motor patterns and produces the wrong result.

### No keyboard access to menu actions

Actions that exist only as toolbar buttons or right-click context menu items, with no keyboard shortcut and no menu bar entry, are inaccessible to keyboard-only users. Every significant action in the application must be in the menu bar (or have an explicit keyboard shortcut) so it is keyboard-accessible.

### State loss on crash

Desktop apps are expected to survive unexpected interruptions. A word processor that loses unsaved changes when force-quit fails user expectations. macOS's document restoration system handles this automatically for AppKit/SwiftUI apps. Electron and Tauri apps must implement their own autosave and crash recovery — this is a design obligation, not an optional enhancement.

### No session restoration

Opening the application and finding a blank initial state rather than the state the user left is a desktop anti-pattern. Users must re-navigate to their work every session. This is the default for many frameworks — developers must explicitly implement window position restoration, document reopening, and panel layout restoration. The design must specify what is restored and what is intentionally not.

### Electron apps that ignore system appearance

An Electron app that does not respond to macOS's dark mode, ignores the user's accent color, uses a hardcoded font family instead of the system font stack, and displays with Chromium's default scrollbars — this is a design app that looks wrong on every platform it ships on. System appearance integration requires explicit design investment; it is not automatic in Electron.

---

## AI-Agent Adaptations

When an agent applies this skill:

**Platform must be the first design question.** Before specifying any desktop UI, establish which platform(s) the application targets. A design that ignores platform specifics is wrong on all platforms. If cross-platform: decide whether to converge on a single convention or adapt per-platform, and state that decision in the Design Spec.

**Default to platform-convention compliance.** When in doubt, follow the platform HIG. Any deviation must be explicitly justified in the Design Spec's Design Decision section. The anti-pattern to avoid: inventing novel UI patterns that are not from the platform HIG and are not justified by a specific design need.

**Explicitly specify the menu bar structure.** The menu bar is the IA of a desktop application. Leaving it unspecified is leaving a fundamental design artifact undelivered. Spec the menu structure with: which menus exist, what commands are in each, which have keyboard shortcuts, which end with "…" (open a dialog), which execute immediately.

**Produce a keyboard shortcut map.** List the 10–20 primary shortcuts, their modifier+key combinations, their platform-appropriate form (Cmd for macOS, Ctrl for Windows/Linux), and verify none conflict with the platform's reserved shortcut list. This is a deliverable artifact of the design engagement.

**Specify window state persistence explicitly.** Enumerate: which state categories are ephemeral, session, preference, and document. Specify what is restored on relaunch and what the first-launch experience looks like. The developer cannot guess the design intent for what to persist.

**If the application is Electron or Tauri, address native trade-offs explicitly.** Name which native conventions will be implemented (menu bar structure, keyboard shortcuts, system fonts, dark mode), which native behaviors will not be available (specific scrollbar behavior, certain platform services), and the design rationale for each decision.

**Anti-fabrication note:** Claims about specific macOS HIG or GNOME HIG content must be based on the referenced sources, not on training data alone. Cite the HIG URL for specific behavioral claims. The Apple HIG at developer.apple.com/design/human-interface-guidelines/ is the authoritative source; individual page content may require direct verification.

---

## References

| Source | Platform | URL | What it covers |
|---|---|---|---|
| Apple Human Interface Guidelines | macOS | developer.apple.com/design/human-interface-guidelines/ | Authoritative macOS conventions: menu bar, keyboard shortcuts, window management, Cmd key modifiers, accessibility |
| Microsoft Fluent 2 Design System | Windows | fluent2.microsoft.design | Current Windows design system, component conventions, accessibility |
| GNOME Human Interface Guidelines | GNOME Linux | developer.gnome.org/hig/ | GNOME keyboard conventions, CSD (Header Bar), standard shortcuts, navigation, keyboard navigation requirement |
| GNOME HIG — Keyboard Reference | GNOME Linux | developer.gnome.org/hig/reference/keyboard.html | Complete standard shortcut table, modifier conventions, reserved keys |
| Electron Accessibility Documentation | Electron | electronjs.org/docs/latest/tutorial/accessibility | How Electron exposes the accessibility tree; `setAccessibilitySupportEnabled()` requirement |
| Tauri Documentation | Tauri | tauri.app | System webview model, Rust backend, plugin ecosystem (window-state, store) |
| macOS Accessibility API | macOS | developer.apple.com/documentation/accessibility | NSAccessibility protocol, AXUIElement, VoiceOver integration |
| Windows UI Automation | Windows | docs.microsoft.com/ui-automation | UIA control patterns, accessibility for Windows applications |

---
