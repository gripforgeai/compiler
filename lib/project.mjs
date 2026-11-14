/**
 * What the compiler reads from the disk: the project file (`project.gripforge`, or the only `*.gripforge` of a
 * folder), the scene files it still names (an old project not yet in one file; sub-scenes carried in
 * `data.scene_files` need none), and the catalog of the kits (every `kits/<kit>/kit.api.json`). The core
 * (lib/core.mjs) never touches the disk: it gets all of it in one request.
 *
 * Used by lib/build.mjs.
 */
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
/** The catalog of kits the package carries (scripts/prepare.mjs), used when no `kits/` folder is found. */
export const CATALOG_FILE = join(here, '..', 'dist', 'kits.json');

/** An error the command reports as an issue (code, message, fix) rather than a stack. */
export class BuildError extends Error {
  constructor(issue) {
    super(`${issue.code}: ${issue.message}`);
    this.issue = { level: 'error', ...issue };
  }
}

/** The project file a path names: the file, or in a folder `project.gripforge`, else its only `*.gripforge`. */
export function projectFile(path) {
  const p = resolve(path ?? '.');
  if (!existsSync(p)) throw new BuildError({ code: 'project_missing', file: p, message: `${p} does not exist`, fix: 'give the path of project.gripforge or of its folder' });
  if (!statSync(p).isDirectory()) return p;
  if (existsSync(join(p, 'project.gripforge'))) return join(p, 'project.gripforge');
  const all = readdirSync(p).filter((f) => f.endsWith('.gripforge'));
  if (all.length === 1) return join(p, all[0]);
  throw new BuildError({ code: 'project_missing', file: p, message: all.length ? `${p} holds several .gripforge files (${all.join(', ')})` : `${p} holds no project.gripforge`, fix: 'name the project file itself, or write project.gripforge in the folder' });
}

/**
 * Where a text stops being JSON (an offset): the message's `position` when the engine gives one, else the shortest
 * prefix that fails for another reason than its end (a prefix of valid JSON only "ends early"; binary search).
 */
export function errorOffset(text, message) {
  const at = /position (\d+)/.exec(message)?.[1];
  if (at) return Number(at);
  const early = (n) => {
    try {
      JSON.parse(text.slice(0, n));
      return true;
    } catch (e) {
      const p = /position (\d+)/.exec(e.message)?.[1];
      return /end of (JSON )?(input|data)|Unterminated|Unexpected end/i.test(e.message) || (p !== undefined && Number(p) >= n);
    }
  };
  let lo = 0, hi = text.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (early(mid + 1)) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

/** A JSON file, or a build error that says where it is not JSON (line and column). */
export function readJson(file, code = 'file_not_json') {
  const text = readFileSync(file, 'utf8');
  try {
    return JSON.parse(text);
  } catch (e) {
    const at = errorOffset(text, e.message);
    const before = text.slice(0, at).split('\n');
    const where = `line ${before.length}, column ${before.at(-1).length + 1}`;
    throw new BuildError({ code, file, message: `${file} is not JSON (${where}): ${e.message.split('\n')[0]}`, fix: 'fix the JSON syntax (commas, quotes, brackets) at that place' });
  }
}

/** The scene files a project still names (`data.scenes[].file`, scene nodes' `scene`), read from beside it. */
export function sceneFiles(project, dir) {
  const files = {}, carried = project?.data?.scene_files ?? {};
  const visit = (path) => {
    if (typeof path !== 'string' || path in files || path in carried) return;
    const file = join(dir, path);
    if (!existsSync(file)) return; // the core says scene_file_missing, with the node that names it
    files[path] = readJson(file);
    walk(files[path]);
  };
  const walk = (v) => {
    if (Array.isArray(v)) return v.forEach(walk);
    if (!v || typeof v !== 'object') return;
    if (v.type === 'scene') visit(v.scene);
    Object.values(v).forEach(walk);
  };
  for (const s of project?.data?.scenes ?? []) visit(s?.file);
  walk(project?.data?.scenes ?? []);
  walk(Object.values(carried));
  return files;
}

/** The `kits/` folder of a module-kits checkout above `from`, or null. */
export function findKitsDir(from) {
  for (let d = resolve(from); ; d = dirname(d)) {
    const k = join(d, 'kits');
    if (existsSync(k) && statSync(k).isDirectory() && readdirSync(k).some((e) => existsSync(join(k, e, 'kit.api.json')))) return k;
    if (dirname(d) === d) return null;
  }
}

/** The kit.api.json of every kit of a folder (`<dir>/<kit>/kit.api.json`), or of the catalog the package carries (`catalog`). */
export function kitCatalog(dir, catalog = CATALOG_FILE) {
  if (dir) {
    if (!existsSync(dir)) throw new BuildError({ code: 'kits_missing', file: dir, message: `the folder of the kits ${dir} does not exist`, fix: 'give --kits <folder holding <kit>/kit.api.json>' });
    return readdirSync(dir).sort().filter((e) => existsSync(join(dir, e, 'kit.api.json'))).map((e) => readJson(join(dir, e, 'kit.api.json')));
  }
  if (existsSync(catalog)) return readJson(catalog).kits;
  throw new BuildError({ code: 'kits_missing', message: 'no catalog of kits: no kits/ folder above the project and none in the package', fix: 'give --kits <folder of the kits> (a module-kits checkout: kits/)' });
}
