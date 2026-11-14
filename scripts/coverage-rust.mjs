#!/usr/bin/env node
/**
 * The line coverage of the Rust core of the compiler (wasm/src), by its native tests
 * (`cargo test --manifest-path wasm/Cargo.toml`) built with `-C instrument-coverage`, merged and read with the llvm-tools
 * of the toolchain (`rustup component add llvm-tools`). Prints the table of llvm-cov; exits 1 under `--min` (90).
 *
 *   node scripts/coverage-rust.mjs [--min 90]
 */
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, rmSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const pkg = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const root = pkg;
const manifest = ['--manifest-path', join(pkg, 'wasm', 'Cargo.toml')];
const at = process.argv.indexOf('--min');
const min = at > 0 ? Number(process.argv[at + 1]) : 90;
const sysroot = execFileSync('rustc', ['--print', 'sysroot'], { cwd: root, encoding: 'utf8' }).trim();
const host = /host: (\S+)/.exec(execFileSync('rustc', ['-vV'], { cwd: root, encoding: 'utf8' }))[1];
const tools = join(sysroot, 'lib/rustlib', host, 'bin');
if (!existsSync(join(tools, 'llvm-cov'))) {
  console.error(`no llvm-cov in ${tools}: rustup component add llvm-tools`);
  process.exit(2);
}
const base = join(process.env.TMPDIR ?? join(root, 'wasm', 'target'), 'compiler-coverage');
const prof = join(base, 'profraw');
rmSync(prof, { recursive: true, force: true });
mkdirSync(prof, { recursive: true });
const env = { ...process.env, RUSTFLAGS: '-C instrument-coverage', LLVM_PROFILE_FILE: join(prof, '%p-%m.profraw'), CARGO_TARGET_DIR: join(base, 'target'), RUSTC_WRAPPER: '' };
const out = execFileSync('nice', ['-n', '15', 'cargo', 'test', '-j', '4', ...manifest, '--tests', '--no-run', '--message-format=json'], { cwd: root, env, encoding: 'utf8', maxBuffer: 64 << 20 });
const bins = out.split('\n').filter(Boolean).map((l) => JSON.parse(l)).filter((m) => m.reason === 'compiler-artifact' && m.executable && m.target?.name !== 'build-script-build').map((m) => m.executable);
execFileSync('nice', ['-n', '15', 'cargo', 'test', '-j', '4', ...manifest], { cwd: root, env, stdio: 'inherit' });
const data = join(base, 'merged.profdata');
execFileSync(join(tools, 'llvm-profdata'), ['merge', '-sparse', ...readdirSync(prof).map((f) => join(prof, f)), '-o', data]);
const src = join(pkg, 'wasm', 'src');
const args = ['report', '--instr-profile', data, ...bins.flatMap((b, i) => (i ? ['--object', b] : [b])), src];
const table = execFileSync(join(tools, 'llvm-cov'), args, { encoding: 'utf8' });
console.log(table);
const total = table.split('\n').find((l) => l.startsWith('TOTAL'));
const pct = Number(total?.trim().split(/\s+/).at(-4)?.replace('%', ''));
console.log(`lines covered: ${pct}% (minimum ${min}%)`);
process.exit(pct >= min ? 0 : 1);
