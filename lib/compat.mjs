/**
 * What this compiler needs of the kits it builds against, and the clear error when they do not fit. The compiler's
 * core is built from one revision of the kits SDK (module-kits, `sdk.json`); a project is built against a catalog of
 * kits (`kit.api.json`) and, to compile, the SDK's crates (core/kit-guest, core/kit-facade) and its wasm recipe.
 *
 *   - every kit.api.json must speak the schema the core reads (`compat.kitApiSchema`);
 *   - an SDK checkout must hold the files the build uses (`compat.paths`) and carry the project format the core was
 *     built with (`core/scene-format` version, major.minor = `compat.sceneFormat`).
 *
 * A mismatch is the issue `kits_incompatible`: what was found, what this compiler expects, how to fix it.
 * Used by lib/build.mjs and lib/cli.mjs.
 */
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
/** The SDK the package was made with (scripts/prepare.mjs), else the repository's pin. */
export const SDK_FILES = [join(here, '..', 'dist', 'sdk.json'), join(here, '..', 'sdk.json')];

/** `{ repository, rev, compat }` of this compiler. */
export function sdkPin(files = SDK_FILES) {
  const f = files.find((x) => existsSync(x));
  if (!f) throw Error(`the compiler has no sdk.json (${files.join(', ')}): reinstall @gripforge/compiler`);
  return JSON.parse(readFileSync(f, 'utf8'));
}

const issue = (message, fix, extra = {}) => ({ level: 'error', code: 'kits_incompatible', message, fix, ...extra });
const majorMinor = (v) => String(v ?? '').split('.').slice(0, 2).join('.');

/** Issues for the kits of a catalog this compiler cannot read. */
export function checkKits(kits, compat = sdkPin().compat) {
  return kits.filter((k) => k?.schema !== compat.kitApiSchema).map((k) => issue(
    `the kit ${k?.id ?? '(no id)'} describes itself with ${JSON.stringify(k?.schema ?? null)}, this compiler reads ${compat.kitApiSchema}`,
    `use kits made for ${compat.kitApiSchema}, or a version of @gripforge/compiler made for these kits (see the versions of the compiler)`,
    { property: `kits.${k?.id ?? '?'}.schema` },
  ));
}

/** The version of core/scene-format in an SDK checkout (its own, or the workspace's), or null. */
export function sceneFormatVersion(sdkPath) {
  const crate = join(sdkPath, 'core', 'scene-format', 'Cargo.toml');
  if (!existsSync(crate)) return null;
  const own = /^\s*version\s*=\s*"([^"]+)"/m.exec(readFileSync(crate, 'utf8'));
  if (own) return own[1];
  const ws = join(sdkPath, 'Cargo.toml');
  if (!existsSync(ws)) return null;
  const pkg = /\[workspace\.package\]([^[]*)/.exec(readFileSync(ws, 'utf8'));
  return (pkg && /^\s*version\s*=\s*"([^"]+)"/m.exec(pkg[1])?.[1]) ?? null;
}

/** Issues for an SDK checkout this compiler cannot build against. */
export function checkSdk(sdkPath, compat = sdkPin().compat) {
  const missing = compat.paths.filter((p) => !existsSync(join(sdkPath, p)));
  if (missing.length) {
    return [issue(`the kits SDK at ${sdkPath} lacks ${missing.join(', ')}: it is not a module-kits checkout this compiler can build against`,
      'give --sdk <a module-kits checkout> of the kits this compiler is made for (sdk.json of the compiler), or update the compiler', { file: sdkPath })];
  }
  const found = sceneFormatVersion(sdkPath);
  if (majorMinor(found) !== compat.sceneFormat) {
    return [issue(`the kits SDK at ${sdkPath} carries the project format ${found} (core/scene-format), this compiler was built for ${compat.sceneFormat}.x`,
      `use a version of @gripforge/compiler made for the format ${majorMinor(found)}, or kits with the format ${compat.sceneFormat}`, { file: sdkPath })];
  }
  return [];
}
