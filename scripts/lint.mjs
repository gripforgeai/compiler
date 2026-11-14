#!/usr/bin/env node
/**
 * The lint of the JavaScript, without dependencies: every .mjs/.js parses (`node --check`), no trailing space, no tab
 * indentation, a final newline, no `console.log` left in lib/ (the command writes through io), LF line ends. The Rust
 * core is linted by `cargo fmt --check` and `cargo clippy -D warnings` (CI).
 *
 *   node scripts/lint.mjs
 */
import { execFileSync } from 'node:child_process';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(fileURLToPath(new URL('..', import.meta.url)));
const dirs = ['bin', 'lib', 'scripts', 'test'];
const files = [];
const walk = (d) => {
  for (const e of readdirSync(d)) {
    const p = join(d, e);
    if (statSync(p).isDirectory()) { if (e !== 'golden') walk(p); } else if (/\.(mjs|js)$/.test(e)) files.push(p);
  }
};
dirs.forEach((d) => walk(join(root, d)));
const problems = [];
for (const f of files) {
  const rel = relative(root, f);
  try { execFileSync(process.execPath, ['--check', f], { stdio: 'pipe' }); } catch (e) { problems.push(`${rel}: does not parse: ${e.stderr.toString().split('\n').slice(0, 4).join(' ')}`); }
  const text = readFileSync(f, 'utf8');
  if (text.includes('\r')) problems.push(`${rel}: CRLF line ends`);
  if (!text.endsWith('\n')) problems.push(`${rel}: no final newline`);
  text.split('\n').forEach((l, i) => {
    if (/[ \t]+$/.test(l)) problems.push(`${rel}:${i + 1}: trailing space`);
    if (/^\t/.test(l)) problems.push(`${rel}:${i + 1}: tab indentation`);
    if (rel.startsWith('lib') && /\bconsole\.log\(/.test(l)) problems.push(`${rel}:${i + 1}: console.log in lib/ (write through io)`);
  });
}
for (const p of problems) console.error(p);
console.log(`lint: ${files.length} file(s), ${problems.length} problem(s)`);
process.exit(problems.length ? 1 : 0);
