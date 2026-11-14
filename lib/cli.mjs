/**
 * The command line of `gripforge-build` (`gripforge project build`), as a function: `main(argv, io)` answers the exit
 * code (0 built or checked, 1 the project has errors, 2 a wrong command line) and writes through `io.out` / `io.err`,
 * so that the tests run it in process. bin/gripforge-build.mjs only calls it.
 *
 *   gripforge-build [<project.gripforge | folder>] [--out <dir>] [--kits <dir>] [--sdk <module-kits checkout>]
 *                   [--sdk-rev <commit|branch>] [--target native|wasm|all] [--no-compile] [--check]
 *                   [--lenient] [--jobs <n>] [--target-dir <cargo target>] [--json] [--quiet]
 */
import { basename, dirname } from 'node:path';
import { build } from './build.mjs';
import { call } from './core.mjs';
import { BuildError, findKitsDir, kitCatalog, projectFile, readJson, sceneFiles } from './project.mjs';
import { line, report } from './report.mjs';
import { checkKits } from './compat.mjs';

export const USAGE = 'usage: gripforge-build [<project.gripforge | folder>] [--out <dir>] [--kits <dir>] [--sdk <dir>] [--sdk-rev <rev>] [--target native|wasm|all] [--no-compile] [--check] [--lenient] [--jobs <n>] [--target-dir <dir>] [--json] [--quiet]';
const VALUED = ['--out', '--kits', '--sdk', '--sdk-rev', '--target', '--jobs', '--target-dir'];
const FLAGS = ['--no-compile', '--check', '--lenient', '--json', '--quiet', '--help', '-h', '--version'];
const TARGETS = { native: ['native'], wasm: ['wasm'], all: ['native', 'wasm'] };

/** The options of a command line, or `{ error }`. */
export function parse(argv) {
  const opts = { project: null };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (VALUED.includes(a)) {
      if (argv[i + 1] === undefined) return { error: `${a} needs a value` };
      opts[a.slice(2)] = argv[++i];
    } else if (FLAGS.includes(a)) opts[a.replace(/^-+/, '')] = true;
    else if (a.startsWith('-')) return { error: `unknown option ${a}` };
    else if (opts.project === null) opts.project = a;
    else return { error: `one project only (got ${opts.project} and ${a})` };
  }
  opts.targets = TARGETS[opts.target ?? 'all'];
  if (!opts.targets) return { error: `--target is native, wasm or all (got ${opts.target})` };
  if (opts.jobs !== undefined && !(Number(opts.jobs) > 0)) return { error: `--jobs is a number of jobs (got ${opts.jobs})` };
  return opts;
}

/** Only the strict check, nothing written. */
function check(opts) {
  try {
    const file = projectFile(opts.project);
    const p = readJson(file);
    const kits = kitCatalog(opts.kits ?? findKitsDir(dirname(file)));
    const incompatible = checkKits(kits);
    if (incompatible.length) return { ok: false, issues: incompatible, projectName: basename(file), project: file, steps: [], artifacts: {} };
    const a = call({ op: 'check', project: p, files: sceneFiles(p, dirname(file)), kits, lenient: !!opts.lenient });
    return { ok: a.ok, issues: a.issues, projectName: basename(file), project: file, steps: [], artifacts: {} };
  } catch (e) {
    if (!(e instanceof BuildError)) throw e;
    return { ok: false, issues: [e.issue], steps: [], artifacts: {} };
  }
}

/** Runs the command; answers its exit code. */
export async function main(argv, io = { out: (s) => process.stdout.write(`${s}\n`), err: (s) => process.stderr.write(`${s}\n`) }) {
  const opts = parse(argv);
  if (opts.error) {
    io.err(`gripforge-build: ${opts.error}`);
    io.err(USAGE);
    return 2;
  }
  if (opts.help || opts.h) {
    io.out(USAGE);
    return 0;
  }
  if (opts.version) {
    io.out(call({ op: 'version' }).version);
    return 0;
  }
  const result = opts.check ? check(opts) : await build({
    project: opts.project, out: opts.out, kits: opts.kits, sdk: opts.sdk, sdkRev: opts['sdk-rev'], targets: opts.targets,
    compile: !opts['no-compile'], lenient: !!opts.lenient, jobs: opts.jobs ? Number(opts.jobs) : undefined,
    quiet: !!opts.quiet || !!opts.json, targetDir: opts['target-dir'],
  });
  if (opts.json) {
    io.out(JSON.stringify(report(result), null, 1));
  } else {
    for (const i of result.issues) io.err(line(i, result.projectName));
    const errors = result.issues.filter((i) => i.level === 'error').length;
    const warnings = result.issues.length - errors;
    const name = result.projectName ?? 'project';
    if (result.ok) {
      const what = opts.check ? 'checked' : `built in ${result.out}`;
      const steps = result.steps.length ? ` — ${result.steps.map((s) => `${s.name} ${s.ms} ms`).join(', ')}` : '';
      io.err(`${name}: ${what} (${warnings} warning(s))${steps}`);
    } else {
      io.err(`${name}: ${errors} error(s), ${warnings} warning(s) — nothing built; fix the project and build again`);
    }
  }
  return result.ok ? 0 : 1;
}
