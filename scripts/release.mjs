#!/usr/bin/env node
/**
 * Prepares the files of a GitHub release of the compiler, as the CI does on a tag `vX.Y.Z`
 * (.github/workflows/release.yml, which then creates the release with `gh release create`). Nothing goes to the npm
 * registry: the archive installs by its release URL.
 *
 *   node scripts/release.mjs <x.y.z> --tgz <archive of npm pack> --out <dir>
 *
 * Writes in <dir>:
 *   compiler-<x.y.z>.tgz   the package (npm install <its release URL>)
 *   SHA256SUMS            the sha256 of the files attached to the release
 *   release.json          what the release holds, read by tools and by the download page (format: README)
 *   notes.md              the release notes: the annotated tag's message, else the CHANGELOG section, then the sums
 *
 * Environment: GITHUB_SHA (the commit, else git), GITHUB_REPOSITORY (default gripforgeai/compiler).
 */
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const RELEASE_SCHEMA = 'gripforge.compiler-release/1';
const VERSION = /^\d{1,4}\.\d{1,4}\.\d{1,4}$/;
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');

/** The notes of a version: the message of its annotated tag, else its section of CHANGELOG.md (`## [x.y.z]` or `## x.y.z`), else ''. */
export function notesOf(version, { tagMessage = '', changelog = '' } = {}) {
  const fromTag = tagMessage.trim();
  if (fromTag && fromTag !== `v${version}` && fromTag !== version) return fromTag;
  const out = [];
  let on = false;
  const head = new RegExp(`^## \\[?v?${version.replace(/\./g, '\\.')}\\]?(\\s|$)`);
  for (const l of changelog.split('\n')) {
    if (on && /^## /.test(l)) break;
    if (on) out.push(l);
    if (head.test(l)) on = true;
  }
  return out.join('\n').trim();
}

/** The download URL of a release asset. */
export const assetUrl = (repository, version, name) => `https://github.com/${repository}/releases/download/v${version}/${name}`;

/** release.json: the version, its commit and SDK, its files with their size, sha256 and URL, the install line. */
export function manifestOf({ version, commit, date, sdk, files, repository = 'gripforgeai/compiler' }) {
  const withUrl = files.map((f) => ({ ...f, url: assetUrl(repository, version, f.name) }));
  const archive = withUrl.find((f) => f.name.endsWith('.tgz'));
  return {
    schema: RELEASE_SCHEMA,
    version,
    tag: `v${version}`,
    date,
    commit,
    source: `https://github.com/${repository}/tree/v${version}`,
    sdk,
    files: withUrl,
    install: `npm install ${archive.url}`,
  };
}

/** The body of the release: the notes, the install line, the sums. */
export function releaseNotes(notes, manifest, sums) {
  return `${notes || `Version ${manifest.version}.`}

## Install

\`\`\`sh
${manifest.install}
\`\`\`

Made for the kits SDK ${manifest.sdk.repository} at \`${manifest.sdk.rev}\`.

## SHA-256

\`\`\`text
${sums.trim()}
\`\`\`
`;
}

const git = (args) => {
  try { return execFileSync('git', args, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim(); } catch { return ''; }
};

/** Writes the files of the release of `version` in `out` (see the header); answers the manifest. */
export function prepareRelease({ version, tgz, out, env = process.env, now = new Date().toISOString() }) {
  if (!VERSION.test(version ?? '')) throw Error(`not a version x.y.z: ${version}`);
  const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
  if (pkg.version !== version) throw Error(`the tag says ${version}, package.json says ${pkg.version}`);
  if (!tgz || !existsSync(tgz)) throw Error(`no archive: give --tgz <the file of npm pack> (${tgz})`);
  if (!out) throw Error('give --out <folder of the release files>');
  mkdirSync(out, { recursive: true });
  const name = `compiler-${version}.tgz`;
  const archive = readFileSync(tgz);
  writeFileSync(join(out, name), archive);
  const sdkFile = existsSync(join(root, 'dist', 'sdk.json')) ? join(root, 'dist', 'sdk.json') : join(root, 'sdk.json');
  const sdk = JSON.parse(readFileSync(sdkFile, 'utf8'));
  const tagDate = git(['for-each-ref', `refs/tags/v${version}`, '--format=%(taggerdate:iso-strict)%(creatordate:iso-strict)']);
  const repository = env.GITHUB_REPOSITORY || 'gripforgeai/compiler';
  const files = [{ name, size: archive.length, sha256: sha256(archive) }];
  const manifest = manifestOf({
    version,
    commit: env.GITHUB_SHA || git(['rev-parse', 'HEAD']),
    date: tagDate ? new Date(tagDate.slice(0, 25)).toISOString() : now,
    sdk: { repository: sdk.repository, rev: sdk.rev },
    files,
    repository,
  });
  const manifestBytes = Buffer.from(`${JSON.stringify(manifest, null, 1)}\n`);
  writeFileSync(join(out, 'release.json'), manifestBytes);
  const sums = `${sha256(archive)}  ${name}\n${sha256(manifestBytes)}  release.json\n`;
  writeFileSync(join(out, 'SHA256SUMS'), sums);
  const notes = notesOf(version, {
    tagMessage: git(['for-each-ref', `refs/tags/v${version}`, '--format=%(contents)']),
    changelog: existsSync(join(root, 'CHANGELOG.md')) ? readFileSync(join(root, 'CHANGELOG.md'), 'utf8') : '',
  });
  writeFileSync(join(out, 'notes.md'), releaseNotes(notes, manifest, sums));
  return { manifest, files: [name, 'SHA256SUMS', 'release.json'].map((f) => join(out, f)), notes: join(out, 'notes.md') };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const value = (flag) => (args.includes(flag) ? resolve(args[args.indexOf(flag) + 1]) : null);
  const version = args.find((a, i) => !a.startsWith('--') && !['--tgz', '--out'].includes(args[i - 1]))?.replace(/^v/, '');
  try {
    const r = prepareRelease({ version, tgz: value('--tgz'), out: value('--out') });
    console.log(r.files.join('\n'));
  } catch (err) {
    console.error(`release: ${err.message}`);
    process.exit(1);
  }
}
