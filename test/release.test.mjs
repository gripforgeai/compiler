/**
 * The release to the download storage (scripts/release.mjs): the notes, the entry of versions.json, the merge (newest
 * first, never rewritten), the S3 signature (the AWS reference example), and a whole publication against a storage in
 * memory. No network.
 */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { compareVersions, entryOf, latestOf, mergeVersions, notesOf, publish, s3, sign, VERSIONS_SCHEMA } from '../scripts/release.mjs';
import { HERE, tmp } from './helpers.mjs';

const VERSION = JSON.parse(readFileSync(join(HERE, '..', 'package.json'), 'utf8')).version;

test('versions compare as numbers', () => {
  assert.equal(compareVersions('0.10.0', '0.9.9'), 1);
  assert.equal(compareVersions('1.2.3', '1.2.3'), 0);
  assert.equal(compareVersions('1.2.3', '1.3.0'), -1);
});

test('the notes: the tag message, else the CHANGELOG section, else nothing', () => {
  const changelog = '# Changes\n\n## [0.2.0] - 2026-11-20\n\n- two\n\n## 0.1.0\n\n- one\n';
  assert.equal(notesOf('0.2.0', { tagMessage: 'The second.\n', changelog }), 'The second.');
  assert.equal(notesOf('0.2.0', { tagMessage: 'v0.2.0', changelog }), '- two');
  assert.equal(notesOf('0.1.0', { changelog }), '- one');
  assert.equal(notesOf('0.3.0', { changelog }), '');
});

const files = [{ name: 'compiler-0.2.0.tgz', size: 3, sha256: 'aa' }, { name: 'SHA256SUMS', size: 1, sha256: 'bb' }];
const entry = (version, sum = 'aa') => entryOf({ version, commit: 'c0ffee', date: '2026-11-20T00:00:00.000Z', notes: 'n', sdk: { repository: 'r', rev: 'x' }, files: files.map((f) => (f.name.endsWith('.tgz') ? { ...f, name: `compiler-${version}.tgz`, sha256: sum } : f)), publicUrl: 'https://example.test/compiler/' });

test('an entry: tag, paths under the prefix, public links and the install line', () => {
  const e = entry('0.2.0');
  assert.equal(e.tag, 'v0.2.0');
  assert.equal(e.files[0].path, 'compiler/0.2.0/compiler-0.2.0.tgz');
  assert.equal(e.files[0].url, 'https://example.test/compiler/0.2.0/compiler-0.2.0.tgz');
  assert.equal(e.install, 'npm install https://example.test/compiler/0.2.0/compiler-0.2.0.tgz');
  const bare = entryOf({ version: '0.2.0', commit: 'c', date: 'd', notes: '', sdk: {}, files });
  assert.equal(bare.files[0].url, undefined);
  assert.equal(bare.install, undefined);
});

test('versions.json: newest first, the same archive again changes nothing, another archive is refused', () => {
  let { index } = mergeVersions(null, entry('0.1.0'), 'now');
  ({ index } = mergeVersions(index, entry('0.2.0'), 'now'));
  ({ index } = mergeVersions(index, entry('0.1.1'), 'now'));
  assert.equal(index.schema, VERSIONS_SCHEMA);
  assert.deepEqual(index.versions.map((v) => v.version), ['0.2.0', '0.1.1', '0.1.0']);
  assert.equal(index.latest, '0.2.0');
  assert.equal(latestOf(index).version, '0.2.0');
  assert.equal(mergeVersions(index, entry('0.1.0'), 'now').unchanged, true);
  assert.throws(() => mergeVersions(index, entry('0.1.0', 'other'), 'now'), /never rewritten/);
});

test('the S3 signature is the AWS reference one (GET object, signature v4)', () => {
  const h = sign({
    method: 'GET', url: 'https://examplebucket.s3.amazonaws.com/test.txt', region: 'us-east-1',
    accessKeyId: 'AKIAIOSFODNN7EXAMPLE', secretAccessKey: 'wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY',
    headers: { range: 'bytes=0-9' }, now: new Date('2013-05-24T00:00:00Z'),
  });
  assert.equal(h.authorization, 'AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, SignedHeaders=host;range;x-amz-content-sha256;x-amz-date, Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41');
});

/** A storage in memory behind the real client (fetch replaced). */
function memory() {
  const objects = new Map();
  const fetcher = async (url, { method = 'GET', headers, body } = {}) => {
    assert.match(headers.authorization, /^AWS4-HMAC-SHA256 /);
    const key = new URL(url).pathname.replace(/^\/bucket\//, '');
    if (method === 'PUT') { objects.set(key, Buffer.from(body)); return new Response('', { status: 200 }); }
    return objects.has(key) ? new Response(objects.get(key), { status: 200 }) : new Response('', { status: 404 });
  };
  return { objects, store: s3({ endpoint: 'https://s3.example.test', bucket: 'bucket', accessKeyId: 'k', secretAccessKey: 's', fetcher }) };
}

test('a publication writes the archive, its sums, its entry, versions.json and latest.json, then is idempotent', async () => {
  const tgz = join(tmp(), 'pack.tgz');
  writeFileSync(tgz, 'archive');
  const { objects, store } = memory();
  const lines = [];
  const r = await publish({ version: VERSION, tgz, store, env: { GITHUB_SHA: 'abc', GF_COMPILER_PUBLIC_URL: 'https://example.test/compiler' }, log: (l) => lines.push(l), now: '2026-11-20T00:00:00.000Z' });
  assert.equal(r.unchanged, false);
  assert.deepEqual([...objects.keys()].sort(), [`compiler/${VERSION}/SHA256SUMS`, `compiler/${VERSION}/compiler-${VERSION}.tgz`, `compiler/${VERSION}/release.json`, 'compiler/latest.json', 'compiler/versions.json']);
  const index = JSON.parse(objects.get('compiler/versions.json'));
  assert.equal(index.versions[0].commit, 'abc');
  assert.match(objects.get(`compiler/${VERSION}/SHA256SUMS`).toString(), new RegExp(`^[0-9a-f]{64}  compiler-${VERSION}\\.tgz\\n$`));
  assert.equal(JSON.parse(objects.get('compiler/latest.json')).schema, 'gripforge.compiler-latest/1');
  const again = await publish({ version: VERSION, tgz, store, env: {}, log: (l) => lines.push(l) });
  assert.equal(again.unchanged, true);
});

test('a publication refuses a wrong version, a missing archive, a missing storage setting; a dry run writes nothing', async () => {
  await assert.rejects(publish({ version: 'v1', tgz: null, store: null, log: () => {} }), /not a version/);
  await assert.rejects(publish({ version: '99.0.0', tgz: null, store: null, log: () => {} }), /package.json says/);
  await assert.rejects(publish({ version: VERSION, tgz: '/nowhere.tgz', store: null, log: () => {} }), /no archive/);
  assert.throws(() => s3({ endpoint: 'https://x', bucket: 'b' }), /GF_S3_ACCESS_KEY_ID is required/);
  const tgz = join(tmp(), 'pack.tgz');
  writeFileSync(tgz, 'archive');
  const lines = [];
  const r = await publish({ version: VERSION, tgz, store: null, env: {}, log: (l) => lines.push(l) });
  assert.match(lines[0], /dry run/);
  assert.equal(r.index.latest, VERSION);
});
