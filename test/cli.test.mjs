/**
 * The command line (lib/cli.mjs, run in process; bin/gripforge-build.mjs once as a process): options, exit codes,
 * the lines for a human or an AI, the JSON report.
 */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';
import { main, parse } from '../lib/cli.mjs';
import { BIN, KITS, ROOM_DIR, fakeCargo, projectDir, room, scene, tmp, withCargo } from './helpers.mjs';

async function run(argv) {
  const out = [], err = [];
  const code = await main(argv, { out: (s) => out.push(s), err: (s) => err.push(s) });
  return { code, out: out.join('\n'), err: err.join('\n') };
}

test('parse: values, flags, the targets, the mistakes', () => {
  const o = parse(['p', '--out', 'o', '--target', 'native', '--jobs', '2', '--json', '--no-compile']);
  assert.equal(o.project, 'p');
  assert.deepEqual(o.targets, ['native']);
  assert.equal(o.json, true);
  assert.equal(o['no-compile'], true);
  assert.deepEqual(parse([]).targets, ['native', 'wasm']);
  assert.match(parse(['--out']).error, /needs a value/);
  assert.match(parse(['--fly']).error, /unknown option --fly/);
  assert.match(parse(['a', 'b']).error, /one project only/);
  assert.match(parse(['--target', 'moon']).error, /native, wasm or all/);
  assert.match(parse(['--jobs', 'zero']).error, /number of jobs/);
});

test('a wrong command line exits 2 with the usage; --help exits 0; --version prints the core\'s', async () => {
  const bad = await run(['--fly']);
  assert.equal(bad.code, 2);
  assert.match(bad.err, /unknown option --fly[\s\S]*usage: gripforge-build/);
  assert.equal((await run(['--help'])).code, 0);
  const v = await run(['--version']);
  assert.equal(v.code, 0);
  assert.match(v.out, /^\d+\.\d+\.\d+$/);
});

test('--check on the room: exit 0, checked', async () => {
  const r = await run([ROOM_DIR, '--check']);
  assert.equal(r.code, 0, r.err);
  assert.match(r.err, /project\.gripforge: checked \(0 warning\(s\)\)/);
});

test('--check on a broken project: exit 1, one line per error, located, with what is expected', async () => {
  const p = room();
  scene(p, 'level').kits['core.light'].brightness = 2;
  const r = await run([projectDir(p), '--check', '--kits', KITS]);
  assert.equal(r.code, 1);
  assert.match(r.err, /^error section_field_unknown — project\.gripforge › scene level › property kits\.core\.light\.brightness: .* Expected: use one of/m);
  assert.match(r.err, /1 error\(s\), 0 warning\(s\) — nothing built/);
});

test('--check --json, and a project that cannot be read', async () => {
  const r = await run([ROOM_DIR, '--check', '--json']);
  assert.equal(JSON.parse(r.out).ok, true);
  const missing = await run([join(tmp(), 'none'), '--check']);
  assert.equal(missing.code, 1);
  assert.match(missing.err, /project_missing/);
});

test('a build (fake cargo): exit 0 and the steps with their times; --json gives the artifacts', async () => {
  const cargo = fakeCargo();
  const out = tmp();
  const r = await withCargo(cargo.bin, () => run([ROOM_DIR, '--out', out, '--target', 'native', '--jobs', '2', '--target-dir', join(out, 't')]));
  assert.equal(r.code, 0, r.err);
  assert.match(r.err, /built in .* — read \d+ ms, check\+generate \d+ ms, compile native \d+ ms/);
  assert.ok(cargo.calls()[0].args.includes('2'));
  const j = await withCargo(cargo.bin, () => run([ROOM_DIR, '--out', out, '--target', 'native', '--json']));
  assert.ok(JSON.parse(j.out).artifacts.native.library.endsWith('.so') || process.platform !== 'linux');
  const nc = await run([ROOM_DIR, '--out', tmp(), '--no-compile', '--lenient', '--sdk-rev', 'abc1234']);
  assert.equal(nc.code, 0, nc.err);
});

test('the binary runs the same command as a process', () => {
  const r = spawnSync(process.execPath, [BIN, ROOM_DIR, '--check'], { encoding: 'utf8' });
  assert.equal(r.status, 0, r.stderr);
  assert.equal(spawnSync(process.execPath, [BIN, '--fly'], { encoding: 'utf8' }).status, 2);
});
