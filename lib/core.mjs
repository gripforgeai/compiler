/**
 * The Rust core of the compiler (wasm/, crate gripforge-compiler-wasm) loaded from the wasm the package
 * carries (`dist/gripforge_compiler.wasm`): no WASI, no import; a JSON request in its memory, a JSON answer back.
 * The same check and the same packing as the Rust tools (core/scene-format), never rewritten in JavaScript.
 *
 * Used by lib/build.mjs (the command), the tests, and any tool that embeds the compiler (the MCP, the CLI gripforge).
 */
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
/** Where the package keeps its wasm (written by scripts/prepare.mjs). */
export const WASM_FILE = join(here, '..', 'dist', 'gripforge_compiler.wasm');

let instance = null;

/** The core, instantiated once. `file`: another wasm; `fresh`: not the one kept (tests). */
export function loadCore(file = process.env.GRIPFORGE_COMPILER_WASM ?? WASM_FILE, fresh = false) {
  if (instance && !fresh) return instance;
  if (!existsSync(file)) {
    throw Error(`the core of the compiler is missing (${file}): run "npm run prepare" in the package (it builds the wasm from wasm/), or reinstall @gripforge/compiler`);
  }
  const exports = new WebAssembly.Instance(new WebAssembly.Module(readFileSync(file)), {}).exports;
  if (!fresh) instance = exports;
  return exports;
}

/** One request to the core: `{ op, project, files, kits, lenient }` → its answer (JSON). */
export function call(request) {
  const core = loadCore();
  const bytes = new TextEncoder().encode(JSON.stringify(request));
  const ptr = core.gfc_alloc(bytes.length);
  new Uint8Array(core.memory.buffer, ptr, bytes.length).set(bytes);
  const packed = core.gfc_run(ptr, bytes.length);
  const outPtr = Number(packed >> 32n), outLen = Number(packed & 0xffffffffn);
  const text = new TextDecoder().decode(new Uint8Array(core.memory.buffer, outPtr, outLen));
  core.gfc_free(outPtr, outLen);
  return JSON.parse(text);
}
