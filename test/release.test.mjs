/**
 * The files of a GitHub release (scripts/release.mjs): the notes, release.json, SHA256SUMS and the body of the
 * release, written from a packed archive. No network: the workflow creates the release with these files.
 */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { assetUrl, manifestOf, notesOf, prepareRelease, releaseNotes, RELEASE_SCHEMA } from '../scripts/release.mjs';
import { HERE, tmp } from './helpers.mjs';

const VERSION = JSON.parse(readFileSync(join(HERE, '..', 'package.json'), 'utf8')).version;
const sha = (b) => createHash('sha256').update(b).digest('hex');

test('the notes: the tag message, else the CHANGELOG section, else nothing', () => {
  const changelog = '# Changes\n\n## [0.2.0] - 2026-11-20\n\n- two\n\n## 0.1.0\n\n- one\n';
  assert.equal(notesOf('0.2.0', { tagMessage: 'The second.\n', changelog }), 'The second.');
  assert.equal(notesOf('0.2.0', { tagMessage: 'v0.2.0', changelog }), '- two');
  assert.equal(notesOf('0.1.0', { changelog }), '- one');
  assert.equal(notesOf('0.3.0', { changelog }), '');
  assert.equal(notesOf('0.3.0'), '');
});

test('release.json: the version, its files with their release URL, the install line', () => {
  const m = manifestOf({ version: '0.2.0', commit: 'c0ffee', date: 'd', sdk: { repository: 'r', rev: 'x' }, files: [{ name: 'compiler-0.2.0.tgz', size: 3, sha256: 'aa' }] });
  assert.equal(m.schema, RELEASE_SCHEMA);
  assert.equal(m.tag, 'v0.2.0');
  assert.equal(m.files[0].url, 'https://github.com/gripforgeai/compiler/releases/download/v0.2.0/compiler-0.2.0.tgz');
  assert.equal(m.install, `npm install ${m.files[0].url}`);
  assert.equal(assetUrl('o/r', '1.0.0', 'x'), 'https://github.com/o/r/releases/download/v1.0.0/x');
  const body = releaseNotes('', m, 'aa  compiler-0.2.0.tgz\n');
  assert.match(body, /^Version 0\.2\.0\./);
  assert.match(body, /npm install https:\/\/github\.com\/gripforgeai\/compiler\/releases\/download\/v0\.2\.0\/compiler-0\.2\.0\.tgz/);
  assert.match(body, /aa {2}compiler-0\.2\.0\.tgz/);
});

test('the release files: the archive, release.json, SHA256SUMS that match them, the notes', () => {
  const tgz = join(tmp(), 'pack.tgz');
  writeFileSync(tgz, 'archive');
  const out = tmp();
  const r = prepareRelease({ version: VERSION, tgz, out, env: { GITHUB_SHA: 'abc', GITHUB_REPOSITORY: 'o/r' }, now: '2026-11-20T00:00:00.000Z' });
  assert.deepEqual(r.files.map((f) => f.slice(out.length + 1)), [`compiler-${VERSION}.tgz`, 'SHA256SUMS', 'release.json']);
  assert.equal(r.manifest.commit, 'abc');
  assert.match(r.manifest.files[0].url, /^https:\/\/github\.com\/o\/r\/releases\/download\//);
  const sums = readFileSync(join(out, 'SHA256SUMS'), 'utf8').trim().split('\n').map((l) => l.split(/\s+/));
  for (const [hash, name] of sums) assert.equal(sha(readFileSync(join(out, name))), hash, name);
  assert.match(readFileSync(r.notes, 'utf8'), /## SHA-256/);
});

test('the release refuses a wrong version, a missing archive or output folder', () => {
  assert.throws(() => prepareRelease({ version: 'v1' }), /not a version/);
  assert.throws(() => prepareRelease({ version: '99.0.0' }), /package.json says/);
  assert.throws(() => prepareRelease({ version: VERSION, tgz: '/nowhere.tgz' }), /no archive/);
  const tgz = join(tmp(), 'pack.tgz');
  writeFileSync(tgz, 'archive');
  assert.throws(() => prepareRelease({ version: VERSION, tgz }), /--out/);
});
