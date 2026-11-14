/**
 * What the tests of the compiler share: the paths of the SDK (module-kits at the pinned revision, `npm run sdk`), the dev.builder room (one project.gripforge) as a
 * fresh object to break, a temporary folder under TMPDIR, a fake cargo (a node script the tests point CARGO at, so the
 * steps around cargo are tested without compiling), and the golden comparison with a readable diff.
 */
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { kitCatalog } from '../lib/project.mjs';

export const HERE = dirname(fileURLToPath(import.meta.url));
/** The SDK the tests read: module-kits at the revision of sdk.json (`npm run sdk` puts it in .sdk/module-kits). */
export const ROOT = resolve(process.env.GRIPFORGE_SDK_DIR ?? join(HERE, '..', '.sdk', 'module-kits'));
export const KITS = join(ROOT, 'kits');
export const ROOM_DIR = join(ROOT, 'kits/dev_builder/tests/testbed/room');
export const ROOM = join(ROOM_DIR, 'project.gripforge');
export const BIN = join(HERE, '../bin/gripforge-build.mjs');

let catalog = null;
/** The catalog of the repository's kits, read once. */
export const kits = () => (catalog ??= kitCatalog(KITS));
/** The room, a fresh copy. */
export const room = () => JSON.parse(readFileSync(ROOM, 'utf8'));
export const scene = (p, id) => p.data.scenes.find((s) => s.id === id);
export const node = (s, name) => s.nodes.find((n) => n.name === name);

/** A new empty folder under TMPDIR (never on /). */
export const tmp = () => mkdtempSync(join(process.env.TMPDIR ?? tmpdir(), 'gfc-'));

/** A folder holding `project` as project.gripforge (and the files given, by path). */
export function projectDir(project, files = {}) {
  const dir = tmp();
  writeFileSync(join(dir, 'project.gripforge'), typeof project === 'string' ? project : JSON.stringify(project));
  for (const [p, v] of Object.entries(files)) {
    mkdirSync(dirname(join(dir, p)), { recursive: true });
    writeFileSync(join(dir, p), typeof v === 'string' ? v : JSON.stringify(v));
  }
  return dir;
}

/**
 * A fake cargo: records its arguments and environment in `<dir>/calls.jsonl`, then does what `mode` says: `ok`
 * writes the library or wasm where cargo would, `fail` exits 101 with a compiler error, `nosdk` with a manifest error.
 */
export function fakeCargo(mode = 'ok') {
  const dir = tmp();
  const cjs = join(dir, 'cargo.cjs');
  writeFileSync(cjs, `const fs = require('node:fs'), path = require('node:path');
const args = process.argv.slice(2);
fs.appendFileSync(${JSON.stringify(join(dir, 'calls.jsonl'))}, JSON.stringify({ args, cwd: process.cwd(), env: { CARGO_TARGET_DIR: process.env.CARGO_TARGET_DIR, RUSTFLAGS: process.env.RUSTFLAGS ?? null, RUSTC_BOOTSTRAP: process.env.RUSTC_BOOTSTRAP ?? null } }) + '\\n');
const mode = ${JSON.stringify(mode)};
if (mode === 'fail') { console.error('error[E0425]: cannot find value \`x\` in this scope'); console.error('  --> src/init_scene/level.rs:3:5'); process.exit(101); }
if (mode === 'nosdk') { console.error('error: failed to get \`gripforge-kit-facade\` as a dependency of package \`g\`'); process.exit(101); }
const target = process.env.CARGO_TARGET_DIR;
const wasm = args.includes('wasm32-unknown-unknown');
const profile = args[args.indexOf('--profile') + 1];
const out = wasm ? path.join(target, 'wasm32-unknown-unknown', profile) : path.join(target, profile === 'dev' ? 'debug' : profile);
fs.mkdirSync(out, { recursive: true });
fs.writeFileSync(path.join(out, wasm ? 'gfkit_game_main.wasm' : ${JSON.stringify(process.platform === 'win32' ? 'gfkit_game_main.dll' : process.platform === 'darwin' ? 'libgfkit_game_main.dylib' : 'libgfkit_game_main.so')}), 'built');
`);
  const sh = join(dir, 'cargo');
  writeFileSync(sh, `#!/bin/sh\nexec ${JSON.stringify(process.execPath)} ${JSON.stringify(cjs)} "$@"\n`);
  chmodSync(sh, 0o755);
  const calls = () => (existsSync(join(dir, 'calls.jsonl')) ? readFileSync(join(dir, 'calls.jsonl'), 'utf8').trim().split('\n').map((l) => JSON.parse(l)) : []);
  return { bin: sh, calls };
}

/** Runs `fn` with CARGO pointing at `bin`, then gives CARGO back. */
export async function withCargo(bin, fn) {
  const before = process.env.CARGO;
  process.env.CARGO = bin;
  try {
    return await fn();
  } finally {
    if (before === undefined) delete process.env.CARGO;
    else process.env.CARGO = before;
  }
}

/** A line diff of two texts, `-` expected `+` actual, with 2 lines of context; empty when equal. */
export function diff(expected, actual) {
  if (expected === actual) return '';
  const a = expected.split('\n'), b = actual.split('\n');
  const n = a.length, m = b.length;
  const lcs = Array.from({ length: n + 1 }, () => new Uint32Array(m + 1));
  for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) lcs[i][j] = a[i] === b[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
  const ops = [];
  let i = 0, j = 0;
  while (i < n || j < m) {
    if (i < n && j < m && a[i] === b[j]) { ops.push([' ', a[i], i + 1]); i++; j++; }
    else if (j < m && (i >= n || lcs[i][j + 1] >= lcs[i + 1][j])) { ops.push(['+', b[j], i + 1]); j++; }
    else { ops.push(['-', a[i], i + 1]); i++; }
  }
  const keep = ops.map((o, k) => o[0] !== ' ' || ops.slice(Math.max(0, k - 2), k + 3).some((x) => x[0] !== ' '));
  const out = [];
  ops.forEach((o, k) => {
    if (keep[k]) out.push(`${o[0]} ${String(o[2]).padStart(4)} | ${o[1]}`);
    else if (keep[k - 1]) out.push('  ...');
  });
  return out.join('\n');
}

/**
 * Compares `actual` with the golden file `test/golden/<name>`; `UPDATE_GOLDEN=1` writes it instead. Throws with the
 * diff (expected - / actual +) and the command that updates it.
 */
export function golden(name, actual) {
  const file = join(HERE, 'golden', name);
  if (process.env.UPDATE_GOLDEN === '1' || !existsSync(file)) {
    if (process.env.UPDATE_GOLDEN !== '1' && process.env.CI) throw Error(`golden file missing: ${file} (UPDATE_GOLDEN=1 npm test writes it)`);
    mkdirSync(dirname(file), { recursive: true });
    writeFileSync(file, actual);
    return;
  }
  const d = diff(readFileSync(file, 'utf8'), actual);
  if (d) throw Error(`${name} differs from its golden file (- expected, + generated):\n${d}\n\nIf the change is wanted: UPDATE_GOLDEN=1 npm test, and review the diff of test/golden/ in the PR.`);
}
