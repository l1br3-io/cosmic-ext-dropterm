#!/usr/bin/env node
// scripts/extract-completions.mjs — maintainer tooling, never shipped,
// never a build step, never run on a user's machine.
//
// Reduces the pinned withfig/autocomplete corpus to DropTerm's static
// completion subset as two generated TOML tiers (SPEC-suggest-corpus):
//   data/completions.toml          the core tier (the embedded floor)
//   data/completions-extended.toml every other spec with a non-empty
//                                 static subset (installed data file)
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
//     --out-extended data/completions-extended.toml
//   git diff --exit-code data/   # the second run must be diff-clean
//
// Zero npm dependencies: node:fs, node:path, node:crypto, node:url,
// node:child_process (tar). Anything function-valued in a spec is
// dropped BY TYPE before a key is ever looked at; generators, script,
// and loadSpec functions are never invoked.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync } from "node:fs";
import { dirname } from "node:path";
import { pathToFileURL } from "node:url";
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
const importSpec = (name) => {
  const file = `${PACKAGE}/build/${name}.js`;
  if (!existsSync(file)) fail(`no spec bundle for '${name}' in the corpus`);
  return import(pathToFileURL(file).href).then((module) => {
    const spec = module.default;
    if (spec === undefined || typeof spec !== "object") {
      fail(`'${name}' has no default-export spec object (typeof ${typeof spec})`);
    }
    return spec;
  });
};

const probe = async () => {
  await ensureCorpus();
  for (const cli of ["git", "rustc", "mkdir"]) {
    const spec = await importSpec(cli);
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
/// with no long form at all, the first form is the name.
const flagNameAndAliases = (raw) => {
  const forms = asList(raw).map(str).filter(Boolean);
  if (forms.length === 0) return null;
  const longAt = forms.findIndex((form) => form.startsWith("--"));
  const primaryAt = longAt === -1 ? 0 : longAt;
  return [forms[primaryAt], forms.filter((_, at) => at !== primaryAt)];
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
const reduceSuggestions = (raw) =>
  asList(raw)
    .filter(isData)
    .map((suggestion) => {
      const name = str(suggestion);
      if (name !== null) return { name };
      if (suggestion && typeof suggestion === "object" && str(suggestion.name) !== null) {
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

const reduceArg = (raw) => {
  if (!isData(raw) || !raw || typeof raw !== "object") return null;
  const name = str(raw.name);
  if (name === null) return null;
  return {
    name,
    ...(str(raw.description) !== null && { description: str(raw.description) }),
    ...(raw.isOptional === true && { optional: true }),
    ...(raw.isVariadic === true && { variadic: true }),
    ...(raw.isCommand === true && { command: true }),
    ...(templateKind(raw.template) !== null && { template: templateKind(raw.template) }),
    ...(reduceSuggestions(raw.suggestions).length > 0 && { suggestions: reduceSuggestions(raw.suggestions) }),
  };
};

const reduceFlag = (raw) => {
  if (!isData(raw) || !raw || typeof raw !== "object") return null;
  const named = flagNameAndAliases(raw.name);
  if (named === null) return null;
  const [name, aliases] = named;
  return {
    name,
    ...(str(raw.description) !== null && { description: str(raw.description) }),
    ...(aliases.length > 0 && { aliases }),
    ...(raw.isPersistent === true && { persistent: true }),
    ...(asList(raw.exclusiveOn).map(str).filter(Boolean).length > 0 && {
      exclusive_on: asList(raw.exclusiveOn).map(str).filter(Boolean),
    }),
    ...(raw.hidden === true && { hidden: true }),
    ...(reducePriority(raw.priority) !== null && { priority: reducePriority(raw.priority) }),
    ...(asList(raw.args).map(reduceArg).filter(Boolean).length > 0 && {
      args: asList(raw.args).map(reduceArg).filter(Boolean),
    }),
  };
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
  const sibling = await import(pathToFileURL(file).href).then((m) => m.default);
  if (sibling === undefined || typeof sibling !== "object") return raw;
  return {
    ...raw,
    subcommands: sibling.subcommands ?? raw.subcommands,
    options: sibling.options ?? raw.options,
    args: sibling.args ?? raw.args,
  };
};

const reduceCommand = async (raw, { resolve = true } = {}) => {
  if (!isData(raw) || !raw || typeof raw !== "object") return null;
  const named = nameAndAliases(raw.name);
  if (named === null) return null;
  const [name, aliases] = named;
  const source = resolve ? await spliceLoadSpec(raw) : raw;
  const subcommands = (
    await Promise.all(asList(source.subcommands).map((sub) => reduceCommand(sub, { resolve })))
  ).filter(Boolean);
  const flags = asList(source.options).map(reduceFlag).filter(Boolean);
  const args = asList(source.args).map(reduceArg).filter(Boolean);
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
    const reduced = await reduceCommand(await importSpec(cli));
    if (reduced === null) fail(`'${cli}' reduced to nothing (no name at the root)`);
    assertStatic(reduced, cli);
    assertFlagNames(reduced, cli);
    console.log(`// ${cli}: ${reduced.subcommands?.length ?? 0} subcommands, ${reduced.flags?.length ?? 0} flags, ${reduced.args?.length ?? 0} args`);
    console.log(JSON.stringify(reduced, null, 2));
  }
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
  fail("nothing to do — pass --probe or --reduce a,b (the emit lands with its task)");
};

main().catch((error) => fail(error?.stack ?? String(error)));
