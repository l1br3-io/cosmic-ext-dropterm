#!/usr/bin/env node
// scripts/extract-completions.mjs — maintainer tooling, never shipped,
// never a build step, never run on a user's machine.
//
// Reduces the pinned withfig/autocomplete corpus to DropTerm's static
// completion subset as two generated TOML tiers (SPEC-suggest-corpus,
// grown by suggest-embed's compressed-floor amendment) and the
// compressed floor the binary embeds:
//   data/completions.toml          the core tier (the embedded floor's
//                                  human-editable form)
//   data/completions.toml.gz       the same bytes gzip-compressed with a
//                                  zeroed mtime — deterministic, the one
//                                  the binary include_bytes!s
//   data/completions-extended.toml every other spec with a non-empty
//                                  static subset (installed data file)
//
// Pins (one version, one sha256, verified before extraction):
//   @withfig/autocomplete@2.692.3 — the published build of
//   aef52acff84c45edde61ae610cc2c964802b9a38 (the seed's own pin).
//   tarball sha256: b07159ef0811bc8cec28e3a3016e6ba9568d774b75b8333dc6a5f2acf7d2c3af
//
// OQ-1 finding (settled 2026-11-07 at the probe): the compiled package
// carries one self-contained ESM bundle per spec at build/<cli>.js,
// each with a default-export spec object (name, subcommands, options,
// args; single objects and arrays both appear). Plain `import()` works
// — the type-stripping fallback over src/*.ts is NOT armed.
//
// Regeneration (maintainer, offline after the one fetch):
//   node scripts/extract-completions.mjs \
//     --version 2.692.3 \
//     --sha256 b07159ef0811bc8cec28e3a3016e6ba9568d774b75b8333dc6a5f2acf7d2c3af \
//     --core scripts/completions-core.txt \
//     --out-core data/completions.toml \
//     --out-extended data/completions-extended.toml \
//     --out-gz data/completions.toml.gz
//   git diff --exit-code data/   # the second run must be diff-clean
//
// Zero npm dependencies: node:fs, node:path, node:crypto, node:url,
// node:zlib, node:child_process (tar). Anything function-valued in a
// spec is dropped BY TYPE before a key is ever looked at; generators,
// script, and loadSpec functions are never invoked.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import { pathToFileURL } from "node:url";
import { gzipSync } from "node:zlib";
import process from "node:process";

const PINNED_VERSION = "2.692.3";
const PINNED_SHA256 = "b07159ef0811bc8cec28e3a3016e6ba9568d774b75b8333dc6a5f2acf7d2c3af";
const TARBALL_URL = `https://registry.npmjs.org/@withfig/autocomplete/-/autocomplete-${PINNED_VERSION}.tgz`;
const SCRATCH = "/tmp/dropterm-extract";
const TARBALL = `${SCRATCH}/autocomplete-${PINNED_VERSION}.tgz`;
const PACKAGE = `${SCRATCH}/package`;

const fail = (message) => {
  console.error(`extract-completions: ${message}`);
  process.exit(1);
};

const args = process.argv.slice(2);
const flag = (name) => {
  const at = args.indexOf(`--${name}`);
  return at === -1 ? undefined : args[at + 1];
};

// One pinned version, one pinned sha256: explicit flags must agree
// with the pins or the run refuses — the recipe stays self-checking.
for (const [flagName, passed, pinned, what] of [
  ["version", flag("version"), PINNED_VERSION, "version"],
  ["sha256", flag("sha256"), PINNED_SHA256, "sha256"],
]) {
  if (passed !== undefined && passed !== pinned) {
    fail(`--${flagName} ${passed} disagrees with the pinned ${what} ${pinned}`);
  }
}

const fetchTarball = async () => {
  process.stdout.write(`fetching ${TARBALL_URL} …\n`);
  const response = await fetch(TARBALL_URL);
  if (!response.ok) fail(`fetch failed: HTTP ${response.status}`);
  const { writeFile } = await import("node:fs/promises");
  await writeFile(TARBALL, Buffer.from(await response.arrayBuffer()));
};

const sha256 = (path) =>
  createHash("sha256").update(readFileSync(path)).digest("hex");

/// Makes SCRATCH hold the verified, extracted `package/` tree. The
/// fetch is the only network access; it happens at most once because
/// the tarball is kept in the scratch dir for offline re-runs.
const ensureCorpus = async () => {
  // `package/package.json` marks a complete extraction — a partial
  // peek at three bundles must not pose as the corpus.
  if (existsSync(`${PACKAGE}/package.json`)) return;
  mkdirSync(SCRATCH, { recursive: true });
  if (!existsSync(TARBALL)) await fetchTarball();
  const digest = sha256(TARBALL);
  if (digest !== PINNED_SHA256) {
    fail(`tarball sha256 ${digest} != pinned ${PINNED_SHA256} — refusing to extract`);
  }
  const tar = spawnSync("tar", ["xzf", TARBALL, "-C", SCRATCH], {
    stdio: "inherit",
  });
  if (tar.status !== 0) fail(`tar exited ${tar.status}`);
};

/// Imports one spec bundle. Plain ESM: the default export is the spec
/// object (OQ-1). Module top-level evaluation is the only code that
/// ever runs — nothing the spec exports is called.
const importSpec = async (name) => {
  const file = `${PACKAGE}/build/${name}.js`;
  if (!existsSync(file)) throw new Error(`no spec bundle for '${name}' in the corpus`);
  const spec = await import(pathToFileURL(file).href).then((module) => module.default);
  if (spec === undefined || typeof spec !== "object") {
    throw new Error(`'${name}' has no default-export spec object (typeof ${typeof spec})`);
  }
  return spec;
};

const probe = async () => {
  await ensureCorpus();
  for (const cli of ["git", "rustc", "mkdir"]) {
    const spec = await importSpec(cli).catch((error) => fail(error.message));
    const count = (list) =>
      Array.isArray(list) ? list.length : list === undefined ? 0 : 1;
    console.log(
      `${cli}: name=${spec.name} subcommands=${count(spec.subcommands)} ` +
        `flags=${count(spec.options)} args=${count(spec.args)}`,
    );
  }
};

// --- the reducer: DropTerm's static subset, by type -----------------
//
// The engine-rea §5 census, mechanical: keep only the keys the live
// engine reads, mapped to the suggest-schema's names; any value whose
// JS type is `function` is dropped before a key is even looked at —
// the type check is the boundary, not a key list.

const isData = (value) => typeof value !== "function";
const asList = (value) => (Array.isArray(value) ? value : value === undefined ? [] : [value]);
const str = (value) =>
  typeof value === "string" ? value : typeof value === "number" ? String(value) : null;

/// A name that may be a single string or an array of forms: the first
/// element is the primary `name`, the rest become `aliases`.
const nameAndAliases = (raw) => {
  const forms = asList(raw).map(str).filter(Boolean);
  return forms.length === 0 ? null : [forms[0], forms.slice(1)];
};

/// A flag's names: the first `--` long form is the primary `name`,
/// every other form an alias (short forms recognized, never offered);
/// with no long form at all, the first dashed form is the name. A
/// form without its dash can never be typed at the flag seat, so only
/// dashed forms seat — an option with none (upstream data bugs like
/// aliases.ts's bare `name` option) drops whole, noted.
const flagNameAndAliases = (raw) => {
  const forms = asList(raw).map(str).filter((form) => form && form.startsWith("-"));
  if (forms.length === 0) {
    if (raw !== undefined) noteDrop(raw);
    return null;
  }
  const longAt = forms.findIndex((form) => form.startsWith("--"));
  const primaryAt = longAt === -1 ? 0 : longAt;
  return [forms[primaryAt], forms.filter((_, at) => at !== primaryAt)];
};

/// One note per unique undashed form: pathological specs (simctl's
/// generated tree repeats the same trio ~31k times) must not flood
/// the transcript.
const notedDrops = new Set();
const noteDrop = (raw) => {
  const key = JSON.stringify(raw);
  if (notedDrops.has(key)) return;
  notedDrops.add(key);
  process.stderr.write(`note: option with no dashed form (${key}) dropped\n`);
};

/// The two template kinds whose candidates the Path provider already
/// owns; an array names kinds in preference order, so the first kept
/// kind wins. Every other kind (`help`, `history`, …) drops.
const templateKind = (raw) => {
  for (const kind of asList(raw).map(str)) {
    if (kind === "filepaths" || kind === "folders") return kind;
  }
  return null;
};

/// Literal suggestions: strings become entries, objects keep `name`
/// (+ `description`), everything else — functions, in particular —
/// drops. Not a suggestion survives as data unless it has a name.
const reduceSuggestions = (raw, budget, root) =>
  asList(raw)
    .filter(isData)
    .map((suggestion) => {
      const name = str(suggestion);
      if (name !== null) {
        chargeNode(budget, root);
        return { name };
      }
      if (suggestion && typeof suggestion === "object" && str(suggestion.name) !== null) {
        chargeNode(budget, root);
        return {
          name: str(suggestion.name),
          ...(str(suggestion.description) !== null && { description: str(suggestion.description) }),
        };
      }
      return null;
    })
    .filter(Boolean);

/// `priority` seats as a u8: Fig's negative de-priorities have no
/// seat, so out-of-range integers drop (never clamp — ordering lies).
const reducePriority = (raw) =>
  typeof raw === "number" && Number.isInteger(raw) && raw >= 0 && raw <= 255 ? raw : null;

const reduceArg = (raw, budget, root) => {
  if (!isData(raw) || !raw || typeof raw !== "object") return null;
  const name = str(raw.name);
  if (name === null) return null;
  chargeNode(budget, root);
  const suggestions = reduceSuggestions(raw.suggestions, budget, root);
  return {
    name,
    ...(str(raw.description) !== null && { description: str(raw.description) }),
    ...(raw.isOptional === true && { optional: true }),
    ...(raw.isVariadic === true && { variadic: true }),
    ...(raw.isCommand === true && { command: true }),
    ...(templateKind(raw.template) !== null && { template: templateKind(raw.template) }),
    ...(suggestions.length > 0 && { suggestions }),
  };
};

const reduceFlag = (raw, budget, root) => {
  if (!isData(raw) || !raw || typeof raw !== "object") return null;
  const named = flagNameAndAliases(raw.name);
  if (named === null) return null;
  chargeNode(budget, root);
  const [name, aliases] = named;
  const args = asList(raw.args).map((arg) => reduceArg(arg, budget, root)).filter(Boolean);
  const exclusiveOn = asList(raw.exclusiveOn).map(str).filter(Boolean);
  return {
    name,
    ...(str(raw.description) !== null && { description: str(raw.description) }),
    ...(aliases.length > 0 && { aliases }),
    ...(raw.isPersistent === true && { persistent: true }),
    ...(exclusiveOn.length > 0 && { exclusive_on: exclusiveOn }),
    ...(raw.hidden === true && { hidden: true }),
    ...(reducePriority(raw.priority) !== null && { priority: reducePriority(raw.priority) }),
    ...(args.length > 0 && { args }),
  };
};

/// A per-root node budget (the corpus record's own budget precedent —
/// kitty's 320 MB store, xterm.js's pixel cap): a spec whose reduced
/// tree exceeds it is pathological generated data (simctl's 19 KB
/// bundle expands at evaluation into a ~100k-node device matrix) and
/// drops whole, listed in the tier's header — mechanically, no names.
const NODE_BUDGET = 10_000;

class OversizeError extends Error {
  constructor(name) {
    super(`reduced tree exceeds the ${NODE_BUDGET}-node budget`);
    this.specName = name;
  }
}

const chargeNode = (budget, name) => {
  budget.left -= 1;
  if (budget.left < 0) throw new OversizeError(name);
};

/// String-form `loadSpec` resolves statically, exactly one level
/// (OQ-3): the named sibling module's subcommands/options/args splice
/// in place of the stub's, and nothing the sibling itself loads
/// resolves any deeper — a loadSpec inside resolved content stays a
/// name-and-description stub, whatever its form.
const spliceLoadSpec = async (raw) => {
  if (typeof raw.loadSpec !== "string") return raw;
  const file = `${PACKAGE}/build/${raw.loadSpec}.js`;
  if (!existsSync(file)) {
    process.stderr.write(`note: loadSpec '${raw.loadSpec}' has no sibling bundle — stub kept\n`);
    return raw;
  }
  const sibling = await import(pathToFileURL(file).href)
    .then((m) => m.default)
    .catch((error) => {
      process.stderr.write(`note: loadSpec '${raw.loadSpec}' failed to load (${error.message}) — stub kept\n`);
      return undefined;
    });
  if (sibling === undefined || typeof sibling !== "object") return raw;
  return {
    ...raw,
    subcommands: sibling.subcommands ?? raw.subcommands,
    options: sibling.options ?? raw.options,
    args: sibling.args ?? raw.args,
  };
};

const reduceCommand = async (raw, { resolve = true, budget = { left: NODE_BUDGET }, root = "?" } = {}) => {
  if (!isData(raw) || !raw || typeof raw !== "object") return null;
  const named = nameAndAliases(raw.name);
  if (named === null) return null;
  chargeNode(budget, root);
  const [name, aliases] = named;
  const source = resolve ? await spliceLoadSpec(raw) : raw;
  // One level exactly: children of spliced-in content never resolve.
  const childResolve = resolve && source === raw;
  const subcommands = (
    await Promise.all(
      asList(source.subcommands).map((sub) => reduceCommand(sub, { resolve: childResolve, budget, root })),
    )
  ).filter(Boolean);
  const flags = asList(source.options).map((option) => reduceFlag(option, budget, root)).filter(Boolean);
  const args = asList(source.args).map((arg) => reduceArg(arg, budget, root)).filter(Boolean);
  return {
    name,
    ...(str(raw.description) !== null && { description: str(raw.description) }),
    ...(aliases.length > 0 && { aliases }),
    ...(raw.hidden === true && { hidden: true }),
    ...(reducePriority(raw.priority) !== null && { priority: reducePriority(raw.priority) }),
    ...(raw.filterStrategy === "fuzzy" && { filter_strategy: "fuzzy" }),
    ...(subcommands.length > 0 && { subcommands }),
    ...(flags.length > 0 && { flags }),
    ...(args.length > 0 && { args }),
  };
};

// --- the invariants: nothing function-valued survives, every flag
// name carries its dash — asserted on every reduced tree, loudly. ----

const assertStatic = (value, path) => {
  if (typeof value === "function") fail(`invariant broken: a function survived at ${path}`);
  if (value && typeof value === "object") {
    for (const [key, inner] of Object.entries(value)) assertStatic(inner, `${path}.${key}`);
  }
};

const assertFlagNames = (command, path) => {
  for (const flag of command.flags ?? []) {
    if (!String(flag.name).startsWith("-")) {
      fail(`invariant broken: flag '${flag.name}' at ${path} lost its dash`);
    }
  }
  for (const sub of command.subcommands ?? []) assertFlagNames(sub, path);
};

const reduceProbe = async (names) => {
  await ensureCorpus();
  for (const cli of names) {
    const reduced = await reduceCommand(await importSpec(cli).catch((error) => fail(error.message)));
    if (reduced === null) fail(`'${cli}' reduced to nothing (no name at the root)`);
    assertStatic(reduced, cli);
    assertFlagNames(reduced, cli);
    console.log(`// ${cli}: ${reduced.subcommands?.length ?? 0} subcommands, ${reduced.flags?.length ?? 0} flags, ${reduced.args?.length ?? 0} args`);
    console.log(JSON.stringify(reduced, null, 2));
  }
};

// --- the emit: two deterministic TOML tiers --------------------------

const EXTRACTION_DATE = "2026-11-07"; // pinned so regenerations are byte-identical
const CORE_LIST = flag("core") ?? "scripts/completions-core.txt";
const OUT_CORE = flag("out-core") ?? "data/completions.toml";
// The compressed floor (suggest-embed's 2026-11-07 amendment): the
// same coreText bytes under gzip with a zeroed mtime, so the output
// is deterministic run over run and the raw file stays the
// human-diffable form.
const OUT_GZ = flag("out-gz") ?? "data/completions.toml.gz";
const OUT_EXTENDED = flag("out-extended") ?? "data/completions-extended.toml";
// The per-command byte cap (decided 2026-11-07 under the maintainer's
// generous-by-default delegation): ~590 of the extended specs are
// normal-sized; a few dozen auto-generated monsters (mongocli 645 KiB,
// vault 531, pipenv 527 …) carry most of the 15 MiB whole corpus
// would weigh. A 64 KiB cap keeps 532 commands in ~4.6 MiB and skips
// the monsters, listed in the tier's header. One constant to flip.
const COMMAND_BYTE_CAP = 65_536;
// The tier's sanity ceiling post-cap (amended 2026-11-07 from the
// original 3 MiB at the generous-default gate): catches corpus drift
// at a future regeneration, not today's output (~4.6 MiB).
const SIZE_LIMIT = 5 * 1024 * 1024;
// The OQ-3 named bloat pair. Measured 2026-11-07: resolving their
// loadSpec stubs one level (409 aws services, 68 gcloud services,
// ~50 MB of sibling bundles) exhausts a 6 GB V8 heap on its own — so
// they reduce WITHOUT resolution: service names and descriptions
// stay (`aws <Tab>` still lists every service), the depth below each
// service does not. If the tier still exceeds SIZE_LIMIT after that,
// the run fails for a human decision rather than inventing more drops.
const RESOLUTION_SKIP = ["aws", "gcloud"];

/// A TOML basic string: ASCII specials escaped, everything else raw
/// UTF-8 (astral characters stay whole — no surrogate escapes).
const tomlString = (value) => {
  let out = '"';
  for (const ch of String(value)) {
    const code = ch.codePointAt(0);
    if (ch === '"' || ch === "\\") out += `\\${ch}`;
    else if (code === 0x0a) out += "\\n";
    else if (code === 0x0d) out += "\\r";
    else if (code === 0x09) out += "\\t";
    else if (code < 0x20) out += `\\u${code.toString(16).padStart(4, "0")}`;
    else out += ch;
  }
  return `${out}"`;
};

const stringArray = (values) => `[${values.map(tomlString).join(", ")}]`;

const emitArg = (arg, table, lines) => {
  lines.push("", `[[${table}]]`);
  lines.push(`name = ${tomlString(arg.name)}`);
  if (arg.description !== undefined) lines.push(`description = ${tomlString(arg.description)}`);
  if (arg.optional) lines.push("optional = true");
  if (arg.variadic) lines.push("variadic = true");
  if (arg.command) lines.push("command = true");
  if (arg.template !== undefined) lines.push(`template = ${tomlString(arg.template)}`);
  for (const suggestion of arg.suggestions ?? []) {
    lines.push("", `[[${table}.suggestions]]`);
    lines.push(`name = ${tomlString(suggestion.name)}`);
    if (suggestion.description !== undefined) {
      lines.push(`description = ${tomlString(suggestion.description)}`);
    }
  }
};

const emitFlag = (entry, table, lines) => {
  lines.push("", `[[${table}]]`);
  lines.push(`name = ${tomlString(entry.name)}`);
  if (entry.description !== undefined) lines.push(`description = ${tomlString(entry.description)}`);
  if (entry.aliases?.length) lines.push(`aliases = ${stringArray(entry.aliases)}`);
  if (entry.persistent) lines.push("persistent = true");
  if (entry.exclusive_on?.length) lines.push(`exclusive_on = ${stringArray(entry.exclusive_on)}`);
  if (entry.hidden) lines.push("hidden = true");
  if (entry.priority !== undefined) lines.push(`priority = ${entry.priority}`);
  for (const arg of entry.args ?? []) emitArg(arg, `${table}.args`, lines);
};

const emitCommand = (command, table, lines) => {
  lines.push("", `[[${table}]]`);
  lines.push(`name = ${tomlString(command.name)}`);
  if (command.description !== undefined) lines.push(`description = ${tomlString(command.description)}`);
  if (command.aliases?.length) lines.push(`aliases = ${stringArray(command.aliases)}`);
  if (command.hidden) lines.push("hidden = true");
  if (command.priority !== undefined) lines.push(`priority = ${command.priority}`);
  if (command.filter_strategy !== undefined) lines.push(`filter_strategy = ${tomlString(command.filter_strategy)}`);
  for (const sub of command.subcommands ?? []) emitCommand(sub, `${table}.subcommands`, lines);
  for (const entry of command.flags ?? []) emitFlag(entry, `${table}.flags`, lines);
  for (const arg of command.args ?? []) emitArg(arg, `${table}.args`, lines);
};

const serializeTier = (entries, header) => {
  const lines = [...header];
  for (const text of entries) lines.push(text);
  return `${lines.join("\n").replace(/^\n+/, "")}\n`;
};

/// Serializes one reduced command to its `[[commands]]` tables now,
/// so the object tree is transient — a corpus this size keeps every
/// spec's text (small) instead of every spec's tree (not).
const commandText = (command) => {
  const lines = [];
  emitCommand(command, "commands", lines);
  return lines.join("\n").replace(/^\n+/, "");
};

const byName = (a, b) => (a < b ? -1 : a > b ? 1 : 0);

/// Wraps a comma-separated name list to header-comment width.
const wrapNames = (names, indent) => {
  const lines = [];
  let line = "";
  for (const name of names) {
    const piece = `${name}, `;
    if (line.length + piece.length > 76 - indent.length) {
      lines.push(line.replace(/, $/, ""));
      line = "";
    }
    line += piece;
  }
  if (line) lines.push(line.replace(/, $/, ""));
  return lines.map((text) => `${indent}${text}`);
};

const RECIPE = [
  "# Regeneration (maintainer tooling, a one-off, never a build step; a",
  "# second run is diff-clean — `git diff --exit-code data/` after):",
  "#   node scripts/extract-completions.mjs \\",
  `#     --version ${PINNED_VERSION} \\`,
  `#     --sha256 ${PINNED_SHA256} \\`,
  `#     --core ${CORE_LIST} \\`,
  `#     --out-core ${OUT_CORE} \\`,
  `#     --out-extended ${OUT_EXTENDED} \\`,
  `#     --out-gz ${OUT_GZ}`,
  "# The Rust suite is the schema gate (cargo test suggest): if it",
  "# fails, this script is wrong, not the schema.",
];

const SOURCE = [
  `# Source: @withfig/autocomplete@${PINNED_VERSION} (the published build of`,
  "# aef52acff84c45edde61ae610cc2c964802b9a38, MIT), tarball sha256",
  `# ${PINNED_SHA256}, extracted ${EXTRACTION_DATE}.`,
];

const RULES = [
  "# Conversion (the engine-rea §5 census, mechanical, by type): a",
  "# name's first form is primary and the rest aliases (a flag's first",
  "# --long form primary, so short forms stay recognized, never",
  "# offered); descriptions verbatim; persistent, exclusive_on,",
  "# hidden, priority, and filter_strategy (fuzzy) where the spec",
  "# carries them; args keep optional/variadic/command, the",
  "# filepaths/folders templates, and literal suggestions. Generators,",
  "# scripts, and loadSpec functions never cross — data only — and",
  "# the file format structurally rejects executable content (unknown",
  "# keys fail the whole file).",
];

const MIT_NOTICE = [
  "# ---",
  "# The upstream license, preserved as it applies to this derived data:",
  "#",
  "# MIT License",
  "#",
  "# Copyright (c) 2021 Hercules Labs Inc. (Fig)",
  "#",
  "# Permission is hereby granted, free of charge, to any person obtaining a copy",
  '# of this software and associated documentation files (the "Software"), to deal',
  "# in the Software without restriction, including without limitation the rights",
  "# to use, copy, modify, merge, publish, distribute, sublicense, and/or sell",
  "# copies of the Software, and to permit persons to whom the Software is",
  "# furnished to do so, subject to the following conditions:",
  "#",
  "# The above copyright notice and this permission notice shall be included in all",
  "# copies or substantial portions of the Software.",
  "#",
  '# THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR',
  '# IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,',
  "# FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE",
  "# AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER",
  "# LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,",
  "# OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE",
  "# SOFTWARE.",
  "# ---",
];

const coreHeader = () => [
  "# DropTerm's core completions tier (suggest-corpus) — GENERATED",
  "# FILE, do not edit by hand.",
  "#",
  ...RECIPE,
  "#",
  ...SOURCE,
  `# Core list: ${CORE_LIST} (the embedded floor's size is the`,
  "# maintainer's to grow — names go there, never in the script). The",
  "# companion tier, every other spec at the pin, is",
  `# ${OUT_EXTENDED}; the two are disjoint by construction.`,
  "#",
  "# This table is the floor suggest-embed embeds into the binary,",
  "# gzip-compressed (the raw file beside it is the human-editable",
  "# form); copy this file to",
  "#   ~/.config/cosmic/io.github.l1br3_io.CosmicExtDropterm/v1/completions.toml",
  "# only to override the embedded floor — a command you re-declare",
  "# replaces its embedded entry whole, and a command you add sits",
  "# beside it. Entries upstream seats below the root (git's --amend",
  "# under commit) ride at their true seats. Descriptions render in",
  "# the completion menu's dim second column; the ghost ignores",
  "# them.",
  "#",
  ...RULES,
  "#",
  ...MIT_NOTICE,
];

const extendedHeader = ({ skipped, loadFailed, oversize, oversizeBytes, resolutionSkipped }) => [
  "# DropTerm's extended completions tier (suggest-corpus) — GENERATED",
  "# FILE, do not edit by hand.",
  "#",
  ...RECIPE,
  "#",
  ...SOURCE,
  "#",
  "# Every spec at the pin beyond the core list whose static subset is",
  "# non-empty (a root with no subcommands, flags, or literal arg",
  "# suggestions after the drop is skipped and named below).",
  "# suggest-embed installs this file as a data file",
  "# (share/<appid>/completions-extended.toml) behind the",
  "# autosuggestions_completions_extended toggle; it is never embedded",
  "# in the binary.",
  "#",
  ...(skipped.length > 0
    ? [`# Skipped (${skipped.length} — empty static subset after the drop):`, ...wrapNames(skipped, "#   ")]
    : ["# Skipped: none — every non-core spec kept a non-empty subset."]),
  ...(loadFailed.length > 0
    ? ["#", `# Failed to load (${loadFailed.length}):`, ...wrapNames(loadFailed, "#   ")]
    : []),
  ...(oversize.length > 0
    ? [
        "#",
        `# Oversize (${oversize.length} — reduced tree beyond the`,
        `# ${NODE_BUDGET}-node budget, pathological generated device`,
        "# matrices):",
        ...wrapNames(oversize, "#   "),
      ]
    : []),
  ...(oversizeBytes.length > 0
    ? [
        "#",
        `# Over the per-command byte cap (${oversizeBytes.length} — each`,
        `# beyond ${Math.round(COMMAND_BYTE_CAP / 1024)} KiB serialized,`,
        "# auto-generated API dumps):",
        ...wrapNames(oversizeBytes, "#   "),
      ]
    : []),
  ...(resolutionSkipped.length > 0
    ? [
        "#",
        `# loadSpec resolution skipped (OQ-3): ${resolutionSkipped.join(", ")}`,
        "# keep their service names and descriptions; the depth below",
        "# each service does not cross (one-level aws measured at a",
        "# 6 GB heap — the bloat the fallback names).",
      ]
    : []),
  "#",
  ...RULES,
  "#",
  ...MIT_NOTICE,
];

/// A root spec's emptiness after the drop (the spec's own rule): no
/// subcommands, no flags, and no literal arg suggestions.
const isEmptySubset = (command) =>
  !((command.subcommands?.length ?? 0) > 0 ||
    (command.flags?.length ?? 0) > 0 ||
    (command.args ?? []).some((arg) => (arg.suggestions?.length ?? 0) > 0));

/// Runs an async worker over items with a bounded pool; results stay
/// in input order, so the output never depends on timing.
const mapPool = async (items, limit, worker) => {
  const results = new Array(items.length);
  let next = 0;
  await Promise.all(
    Array.from({ length: Math.min(limit, items.length) }, async () => {
      while (next < items.length) {
        const at = next++;
        results[at] = await worker(items[at]);
      }
    }),
  );
  return results;
};

const emit = async () => {
  await ensureCorpus();
  const coreNames = readCoreList(CORE_LIST);
  const rootNames = readdirSync(`${PACKAGE}/build`, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith(".js"))
    .map((entry) => entry.name.slice(0, -3));

  const missing = coreNames.filter((name) => !rootNames.includes(name));
  if (missing.length > 0) fail(`core names absent from the corpus: ${missing.join(", ")}`);

  process.stdout.write(`reducing ${rootNames.length} specs …\n`);
  const coreSet = new Set(coreNames);
  const reduced = new Map(); // name → serialized entry | "empty" | "load-failed"
  await mapPool(rootNames, 16, async (name) => {
    try {
      // The OQ-3 pair reduces stub-only (see RESOLUTION_SKIP).
      const resolve = !RESOLUTION_SKIP.includes(name);
      const command = await reduceCommand(await importSpec(name), { resolve });
      if (command === null) {
        reduced.set(name, "empty");
        process.stderr.write(`reduced ${name} (empty — no seatable root)\n`);
        return;
      }
      assertStatic(command, name);
      assertFlagNames(command, name);
      if (isEmptySubset(command)) {
        reduced.set(name, "empty");
        process.stderr.write(`reduced ${name} (empty subset)\n`);
        return;
      }
      const text = commandText(command);
      // The cap governs the installed extended file's footprint, not
      // the embedded floor: core names ride regardless of size (the
      // embed pin in suggest-embed's tests is their ceiling).
      if (!coreSet.has(name) && Buffer.byteLength(text) > COMMAND_BYTE_CAP) {
        reduced.set(name, "oversize-bytes");
        process.stderr.write(
          `note: '${name}' serializes to ${Math.round(Buffer.byteLength(text) / 1024)} KiB, over the ` +
            `${Math.round(COMMAND_BYTE_CAP / 1024)} KiB command cap — skipped\n`,
        );
        return;
      }
      reduced.set(name, { name, text });
      process.stderr.write(`reduced ${name}\n`);
    } catch (error) {
      if (error instanceof OversizeError) {
        reduced.set(name, "oversize");
        process.stderr.write(`note: '${name}' ${error.message} — skipped\n`);
      } else {
        reduced.set(name, "load-failed");
        process.stderr.write(`note: '${name}' failed to reduce (${error.message}) — skipped\n`);
      }
    }
  });

  for (const name of coreNames) {
    const outcome = reduced.get(name);
    if (outcome === "empty") fail(`core name '${name}' reduced to an empty static subset`);
    if (outcome === "oversize") fail(`core name '${name}' exceeds the node budget`);
    if (outcome === "load-failed" || outcome === undefined) fail(`core name '${name}' failed to reduce`);
  }

  const byTier = (names) =>
    names.map((name) => reduced.get(name)).sort((a, b) => byName(a.name, b.name));
  const core = byTier(coreNames);
  const seated = (name) => {
    const outcome = reduced.get(name);
    return outcome !== "empty" && outcome !== "load-failed" && outcome !== "oversize" &&
      outcome !== "oversize-bytes";
  };
  const extendedNames = rootNames
    .filter((name) => !coreNames.includes(name))
    .filter(seated)
    .sort(byName);
  const skipped = rootNames.filter((name) => reduced.get(name) === "empty").sort(byName);
  const loadFailed = rootNames.filter((name) => reduced.get(name) === "load-failed").sort(byName);
  const oversize = rootNames.filter((name) => reduced.get(name) === "oversize").sort(byName);
  const oversizeBytes = rootNames
    .filter((name) => reduced.get(name) === "oversize-bytes")
    .sort(byName);

  const overlap = coreNames.filter((name) => extendedNames.includes(name));
  if (overlap.length > 0) fail(`names in both tiers: ${overlap.join(", ")}`);

  // The size gate (OQ-3): the pair already reduced stub-only; if the
  // tier still exceeds the ceiling, the run fails for a human
  // decision — no invented drops.
  const resolutionSkipped = RESOLUTION_SKIP.filter((name) => extendedNames.includes(name));
  const extended = extendedNames;
  const text = serializeTier(
    byTier(extended).map((entry) => entry.text),
    extendedHeader({ skipped, loadFailed, oversize, oversizeBytes, resolutionSkipped }),
  );
  if (Buffer.byteLength(text) > SIZE_LIMIT) {
    const top = byTier(extendedNames)
      .map((entry) => [entry.name, Buffer.byteLength(entry.text)])
      .sort((a, b) => b[1] - a[1])
      .slice(0, 20)
      .map(([name, bytes]) => `${name}=${Math.round(bytes / 1024)}KiB`);
    const sizes = byTier(extendedNames).map((entry) => Buffer.byteLength(entry.text));
    const atCap = (cap) => {
      const kept = sizes.filter((bytes) => bytes <= cap);
      return `${Math.round(sizes.reduce((a, b) => a + b, 0) / 1024 / 102.4) / 10}→${Math.round(kept.reduce((a, b) => a + b, 0) / 1024 / 102.4) / 10} MiB/${kept.length} cmds`;
    };
    fail(
      `extended tier is ${Math.round(Buffer.byteLength(text) / 1024 / 102.4) / 10} MiB over the ` +
        `${SIZE_LIMIT} ceiling with the OQ-3 pair stub-only — top contributors: ${top.join(", ")} ` +
        `— cap simulation (all→kept): 64KiB ${atCap(65536)} | 128KiB ${atCap(131072)} | ` +
        `256KiB ${atCap(262144)} | 512KiB ${atCap(524288)} — needs a human decision`,
    );
  }

  const coreText = serializeTier(core.map((entry) => entry.text), coreHeader());
  writeFileSync(OUT_CORE, coreText);
  // Zeroed mtime: the gzip bytes are a pure function of coreText, so
  // regenerations stay diff-clean and the embed's ≤ 1 MiB pin has a
  // deterministic subject.
  const coreGz = gzipSync(coreText, { mtime: 0 });
  writeFileSync(OUT_GZ, coreGz);
  writeFileSync(OUT_EXTENDED, text);

  const kib = (name) => {
    const entry = reduced.get(name);
    return entry && entry.text ? `${Math.round(Buffer.byteLength(entry.text) / 1024)} KiB` : "absent";
  };
  console.log(
    `core: ${core.length} commands → ${OUT_CORE} (${Math.round(Buffer.byteLength(coreText) / 1024)} KiB)`,
  );
  console.log(
    `core compressed → ${OUT_GZ} (${coreGz.length} bytes, the embedded floor)`,
  );
  console.log(
    `extended: ${extended.length} commands → ${OUT_EXTENDED} (${Math.round((Buffer.byteLength(text) / 1024 / 1024) * 100) / 100} MiB)` +
      ` | aws ${kib("aws")}, gcloud ${kib("gcloud")}`,
  );
  console.log(
    `skipped (empty subset): ${skipped.length} | failed to load: ${loadFailed.length} | oversize: ${oversize.length} | over byte cap: ${oversizeBytes.length}`,
  );
  if (resolutionSkipped.length > 0) {
    console.log(`loadSpec resolution skipped (OQ-3): ${resolutionSkipped.join(", ")}`);
  }
};

const readCoreList = (path) => {
  const names = readFileSync(path, "utf8")
    .split("\n")
    .map((line) => line.replace(/#.*$/, "").trim())
    .filter(Boolean);
  const seen = new Set();
  for (const name of names) {
    if (seen.has(name)) fail(`duplicate core name '${name}' in ${path}`);
    seen.add(name);
  }
  return names;
};

const main = async () => {
  if (args.includes("--probe")) {
    await probe();
    return;
  }
  const reduce = flag("reduce");
  if (reduce !== undefined) {
    await reduceProbe(reduce.split(",").map((name) => name.trim()).filter(Boolean));
    return;
  }
  await emit();
};

main().catch((error) => fail(error?.stack ?? String(error)));
