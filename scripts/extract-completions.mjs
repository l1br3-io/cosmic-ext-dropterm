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
  if (existsSync(PACKAGE)) return;
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

const main = async () => {
  if (args.includes("--probe")) {
    await probe();
    return;
  }
  fail("nothing to do — pass --probe (the reducer and the emit land with their tasks)");
};

main().catch((error) => fail(error?.stack ?? String(error)));
