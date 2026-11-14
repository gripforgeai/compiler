#!/usr/bin/env node
/**
 * Puts a version of the compiler on the download storage (an S3-compatible bucket), as the CI does on a tag `vX.Y.Z`
 * (.github/workflows/release.yml). Nothing goes to the npm registry: the archive installs by its URL.
 *
 *   node scripts/release.mjs <x.y.z> --tgz <archive of npm pack> [--dry-run]
 *
 * Writes, under the prefix (default `compiler`):
 *   compiler/<x.y.z>/compiler-<x.y.z>.tgz   the package (`npm i <url of this file>`)
 *   compiler/<x.y.z>/SHA256SUMS            its sha256
 *   compiler/<x.y.z>/release.json          the entry of this version (the format of versions.json, README)
 *   compiler/versions.json                 every version, newest first (read, merged, written back)
 *   compiler/latest.json                   the newest version's entry (only when this version is the newest)
 * A version is never rewritten: the same archive again changes nothing, another archive under the same version is
 * refused. Each file written is read back and compared.
 *
 * Environment: GF_S3_ENDPOINT, GF_S3_BUCKET, GF_S3_REGION (default "garage"), GF_S3_ACCESS_KEY_ID,
 * GF_S3_SECRET_ACCESS_KEY, GF_S3_PREFIX (default "compiler"), GF_COMPILER_PUBLIC_URL (optional: the public base the
 * files are downloaded from, written as `url` of each file), GITHUB_SHA (the commit, else git), GITHUB_REPOSITORY.
 */
import { createHash, createHmac } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const VERSIONS_SCHEMA = 'gripforge.compiler-versions/1';
export const LATEST_SCHEMA = 'gripforge.compiler-latest/1';
const VERSION = /^\d{1,4}\.\d{1,4}\.\d{1,4}$/;
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
const hmac = (key, text) => createHmac('sha256', key).update(text).digest();

/** -1, 0, 1: x.y.z compared as numbers. */
export function compareVersions(a, b) {
  const x = a.split('.').map(Number), y = b.split('.').map(Number);
  for (let i = 0; i < 3; i++) if (x[i] !== y[i]) return x[i] < y[i] ? -1 : 1;
  return 0;
}

/** The notes of a version: the message of its annotated tag, else its section of CHANGELOG.md (`## [x.y.z]` or `## x.y.z`), else ''. */
export function notesOf(version, { tagMessage = '', changelog = '' } = {}) {
  const fromTag = tagMessage.trim();
  if (fromTag && fromTag !== `v${version}` && fromTag !== version) return fromTag;
  const lines = changelog.split('\n'), out = [];
  let on = false;
  const head = new RegExp(`^## \\[?v?${version.replace(/\./g, '\\.')}\\]?(\\s|$)`);
  for (const l of lines) {
    if (on && /^## /.test(l)) break;
    if (on) out.push(l);
    if (head.test(l)) on = true;
  }
  return out.join('\n').trim();
}

/** The entry of one version in versions.json. */
export function entryOf({ version, commit, date, notes, sdk, files, prefix = 'compiler', publicUrl = null, repository = 'gripforgeai/compiler' }) {
  const withUrl = files.map((f) => ({ ...f, path: `${prefix}/${version}/${f.name}`, ...(publicUrl ? { url: `${publicUrl.replace(/\/$/, '')}/${version}/${f.name}` } : {}) }));
  const archive = withUrl.find((f) => f.name.endsWith('.tgz'));
  return {
    version,
    tag: `v${version}`,
    date,
    commit,
    source: `https://github.com/${repository}/tree/v${version}`,
    notes,
    sdk,
    files: withUrl,
    ...(archive?.url ? { install: `npm install ${archive.url}` } : {}),
  };
}

/**
 * versions.json with `entry` in it, newest first. The same version again: kept if its archive is the same
 * (`unchanged: true`), refused otherwise. `latest`: the newest version.
 */
export function mergeVersions(index, entry, now) {
  const versions = [...(index?.versions ?? [])];
  const at = versions.findIndex((v) => v.version === entry.version);
  if (at >= 0) {
    const sum = (e) => e.files.find((f) => f.name.endsWith('.tgz'))?.sha256;
    if (sum(versions[at]) !== sum(entry)) throw Error(`version ${entry.version} is already published with another archive (sha256 ${sum(versions[at])}): a version is never rewritten, release a new one`);
    return { index, unchanged: true };
  }
  versions.push(entry);
  versions.sort((a, b) => compareVersions(b.version, a.version));
  return { index: { schema: VERSIONS_SCHEMA, updated: now, latest: versions[0].version, versions }, unchanged: false };
}

/** latest.json: the newest version's entry. */
export const latestOf = (index) => ({ schema: LATEST_SCHEMA, ...index.versions[0] });

/**
 * The AWS signature v4 headers of one S3 request (path-style). `headers`: extra headers to sign (lower-case names).
 */
export function sign({ method, url, region, accessKeyId, secretAccessKey, body = Buffer.alloc(0), headers = {}, now = new Date() }) {
  const u = new URL(url);
  const amzDate = now.toISOString().replace(/[-:]/g, '').replace(/\.\d{3}/, '');
  const day = amzDate.slice(0, 8);
  const payload = sha256(body);
  const all = { host: u.host, 'x-amz-content-sha256': payload, 'x-amz-date': amzDate, ...headers };
  const names = Object.keys(all).map((n) => n.toLowerCase()).sort();
  const canonicalHeaders = names.map((n) => `${n}:${String(all[n]).trim()}\n`).join('');
  const query = [...u.searchParams].sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)).map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(v)}`).join('&');
  const canonical = [method, u.pathname.split('/').map((s) => encodeURIComponent(decodeURIComponent(s))).join('/'), query, canonicalHeaders, names.join(';'), payload].join('\n');
  const scope = `${day}/${region}/s3/aws4_request`;
  const toSign = ['AWS4-HMAC-SHA256', amzDate, scope, sha256(canonical)].join('\n');
  const key = hmac(hmac(hmac(hmac(`AWS4${secretAccessKey}`, day), region), 's3'), 'aws4_request');
  const signature = createHmac('sha256', key).update(toSign).digest('hex');
  return { ...all, authorization: `AWS4-HMAC-SHA256 Credential=${accessKeyId}/${scope}, SignedHeaders=${names.join(';')}, Signature=${signature}` };
}

/** A tiny S3 client (GET, PUT) over fetch, path-style. */
export function s3({ endpoint, bucket, region = 'garage', accessKeyId, secretAccessKey, fetcher = fetch }) {
  for (const [k, v] of Object.entries({ GF_S3_ENDPOINT: endpoint, GF_S3_BUCKET: bucket, GF_S3_ACCESS_KEY_ID: accessKeyId, GF_S3_SECRET_ACCESS_KEY: secretAccessKey })) {
    if (!v) throw Error(`${k} is required to publish (a variable or secret of the release workflow)`);
  }
  const url = (key) => `${endpoint.replace(/\/$/, '')}/${bucket}/${key}`;
  return {
    async get(key) {
      const r = await fetcher(url(key), { headers: sign({ method: 'GET', url: url(key), region, accessKeyId, secretAccessKey }) });
      if (r.status === 404) return null;
      if (!r.ok) throw Error(`GET ${key}: the storage answered ${r.status}`);
      return Buffer.from(await r.arrayBuffer());
    },
    async put(key, body, type) {
      const headers = sign({ method: 'PUT', url: url(key), region, accessKeyId, secretAccessKey, body, headers: { 'content-type': type } });
      const r = await fetcher(url(key), { method: 'PUT', headers, body });
      if (!r.ok) throw Error(`PUT ${key}: the storage answered ${r.status} ${(await r.text()).slice(0, 200)}`);
    },
  };
}

const json = (v) => Buffer.from(`${JSON.stringify(v, null, 1)}\n`);
const git = (args) => {
  try { return execFileSync('git', args, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim(); } catch { return ''; }
};

/** Publishes one version (see the header). `store`: an s3() client, null for a dry run. */
export async function publish({ version, tgz, store, env = process.env, log = console.log, now = new Date().toISOString() }) {
  if (!VERSION.test(version)) throw Error(`not a version x.y.z: ${version}`);
  const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
  if (pkg.version !== version) throw Error(`the tag says ${version}, package.json says ${pkg.version}`);
  if (!tgz || !existsSync(tgz)) throw Error(`no archive: give --tgz <the file of npm pack> (${tgz})`);
  const prefix = env.GF_S3_PREFIX || 'compiler';
  const name = `compiler-${version}.tgz`;
  const archive = readFileSync(tgz);
  const sums = Buffer.from(`${sha256(archive)}  ${name}\n`);
  const sdk = JSON.parse(readFileSync(existsSync(join(root, 'dist', 'sdk.json')) ? join(root, 'dist', 'sdk.json') : join(root, 'sdk.json'), 'utf8'));
  const tagDate = git(['for-each-ref', `refs/tags/v${version}`, '--format=%(taggerdate:iso-strict)%(creatordate:iso-strict)']);
  const entry = entryOf({
    version,
    commit: env.GITHUB_SHA || git(['rev-parse', 'HEAD']),
    date: tagDate ? new Date(tagDate.slice(0, 25)).toISOString() : now,
    notes: notesOf(version, { tagMessage: git(['for-each-ref', `refs/tags/v${version}`, '--format=%(contents)']), changelog: existsSync(join(root, 'CHANGELOG.md')) ? readFileSync(join(root, 'CHANGELOG.md'), 'utf8') : '' }),
    sdk: { repository: sdk.repository, rev: sdk.rev },
    files: [
      { name, size: archive.length, sha256: sha256(archive) },
      { name: 'SHA256SUMS', size: sums.length, sha256: sha256(sums) },
    ],
    prefix,
    publicUrl: env.GF_COMPILER_PUBLIC_URL || null,
    repository: env.GITHUB_REPOSITORY || 'gripforgeai/compiler',
  });
  const before = store ? JSON.parse((await store.get(`${prefix}/versions.json`))?.toString() ?? 'null') : null;
  const { index, unchanged } = mergeVersions(before, entry, now);
  if (unchanged) {
    log(`release: ${version} is already published with this archive: nothing to do`);
    return { entry, index, unchanged };
  }
  if (!store) {
    log(`release (dry run): would write ${prefix}/${version}/{${name},SHA256SUMS,release.json}, ${prefix}/versions.json${index.latest === version ? `, ${prefix}/latest.json` : ''}`);
    log(JSON.stringify(entry, null, 1));
    return { entry, index, unchanged };
  }
  const writes = [
    [`${prefix}/${version}/${name}`, archive, 'application/gzip'],
    [`${prefix}/${version}/SHA256SUMS`, sums, 'text/plain; charset=utf-8'],
    [`${prefix}/${version}/release.json`, json(entry), 'application/json'],
    [`${prefix}/versions.json`, json(index), 'application/json'],
    ...(index.latest === version ? [[`${prefix}/latest.json`, json(latestOf(index)), 'application/json']] : []),
  ];
  for (const [key, body, type] of writes) {
    await store.put(key, body, type);
    const back = await store.get(key);
    if (!back || sha256(back) !== sha256(body)) throw Error(`${key}: read back differs from what was written`);
    log(`release: ${key} (${body.length} bytes, sha256 ${sha256(body)})`);
  }
  return { entry, index, unchanged };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const version = args.find((a) => !a.startsWith('--') && args[args.indexOf(a) - 1] !== '--tgz')?.replace(/^v/, '');
  const tgz = args.includes('--tgz') ? resolve(args[args.indexOf('--tgz') + 1]) : null;
  const dry = args.includes('--dry-run');
  const e = process.env;
  const store = dry ? null : s3({ endpoint: e.GF_S3_ENDPOINT, bucket: e.GF_S3_BUCKET, region: e.GF_S3_REGION || 'garage', accessKeyId: e.GF_S3_ACCESS_KEY_ID, secretAccessKey: e.GF_S3_SECRET_ACCESS_KEY });
  publish({ version, tgz, store }).catch((err) => {
    console.error(`release: ${err.message}`);
    process.exit(1);
  });
}
