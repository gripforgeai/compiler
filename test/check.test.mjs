/**
 * Step 2, validation (the core's `check`, strict): the room is clean; each way to break it refuses the build with one
 * error that says the file, the scene, the element, the property and what is expected. One row per case below.
 */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { call, loadCore } from '../lib/core.mjs';
import { kits, node, room, scene } from './helpers.mjs';

const check = (project, extra = {}) => call({ op: 'check', project, files: {}, kits: kits(), ...extra });
const errors = (a) => a.issues.filter((i) => i.level === 'error');

test('the room checks clean, its scenes listed with their Rust module', () => {
  const a = check(room());
  assert.equal(a.ok, true, JSON.stringify(a.issues, null, 1));
  assert.deepEqual(a.scenes, [
    { id: 'menu', file: '#menu', module: 'menu' },
    { id: 'level', file: '#level', module: 'level' },
  ]);
  assert.equal(a.errors, 0);
});

/** [what, how the room is broken, the issue expected: code, scene, node, field (a RegExp), message (a RegExp)]. */
const CASES = [
  ['an unknown property of a kit section', (p) => { scene(p, 'level').kits['core.light'].brightness = 2; },
    { code: 'section_field_unknown', scene: 'level', field: /kits\.core\.light\.brightness/, fix: /exposure, preset/ }],
  ['an unknown property of a node', (p) => { node(scene(p, 'level'), 'Sun').light.glow = true; },
    { scene: 'level', node: 'Sun', field: /light/ }],
  ['a value of the wrong type in a node', (p) => { node(scene(p, 'level'), 'Sun').light.intensity = 'strong'; },
    { code: 'prop_invalid', scene: 'level', node: 'Sun', field: /light\.intensity/, fix: /number/ }],
  ['a value of the wrong type in a kit section', (p) => { scene(p, 'level').kits['core.light'].preset = 3; },
    { code: 'section_invalid', scene: 'level' }],
  ['a value out of its bounds', (p) => { scene(p, 'level').kits['core.light'].exposure = -1; },
    { code: 'prop_out_of_range', scene: 'level', field: /kits\.core\.light\.exposure/, message: /at least 0, -1 is written/ }],
  ['a broken reference: a sub-scene that is not there', (p) => { node(scene(p, 'level'), 'Shelf').scene = 'scenes/cupboard.gfscene'; },
    { code: 'scene_file_missing', scene: 'level', node: 'Shelf', message: /cupboard/ }],
  ['a broken reference: a reflection on a node that does not exist', (p) => { node(scene(p, 'level'), 'Door').reflect[0].set = { Handle: { rotation: [0, 95, 0] } }; },
    { scene: 'level', node: 'Door' }],
  ['a broken reference: a binding that loads a scene that does not exist', (p) => { scene(p, 'menu').on[0].do[0].load_scene.scene = 'credits'; },
    { code: 'scene_unknown', scene: 'menu', field: /on\[0\]\.do\[0\]\.load_scene\.scene/, fix: /menu, level/ }],
  ['a broken reference: the next scene does not exist', (p) => { scene(p, 'menu').next = 'credits'; },
    { code: 'scene_unknown', scene: 'menu', field: /^next$/ }],
  ['a circular reference: a sub-scene that instances itself', (p) => { p.data.scene_files['scenes/shelf.gfscene'].nodes.push({ name: 'Again', type: 'scene', scene: 'scenes/shelf.gfscene' }); },
    { code: 'scene_cycle', message: /instances itself/ }],
  ['a kit required by a scene and not installed', (p) => { scene(p, 'menu').requires.push('core.physics'); },
    { code: 'kit_required_missing', scene: 'menu', fix: /core\.physics/ }],
  ['a kit unknown to the catalog', (p) => { p.installed.push({ id: 'world.nowhere' }); },
    { code: 'kit_unknown', field: /installed/, message: /world\.nowhere/ }],
  ['a kit in an incompatible version', (p) => { scene(p, 'menu').requires = ['game.engine', { id: 'core.light', version: '>=9.0.0' }, 'core.camera']; },
    { code: 'kit_version_mismatch', scene: 'menu', message: /core\.light/ }],
  ['an empty scene (no nodes)', (p) => { p.data.scenes.push({ id: 'void', nodes: [] }); },
    { code: 'scene_empty', scene: 'void', message: /nothing in it/ }],
  ['an empty scene (nothing at all)', (p) => { p.data.scenes.push({ id: 'void' }); },
    { code: 'scene_empty', scene: 'void' }],
  ['two nodes with the same name', (p) => { const m = scene(p, 'menu'); m.nodes.push({ ...m.nodes[0] }); },
    { code: 'name_duplicate', scene: 'menu' }],
  ['two scenes with the same id', (p) => { p.data.scenes.push({ ...scene(p, 'menu') }); },
    { code: 'scene_id_duplicate', scene: 'menu' }],
  ['a project without scenes', (p) => { p.data.scenes = []; },
    { code: 'scenes_missing' }],
  ['a node of an unknown type', (p) => { scene(p, 'menu').nodes.push({ name: 'Blob', type: 'blob' }); },
    { code: 'node_type_unknown', scene: 'menu', node: 'Blob' }],
];

for (const [what, breakIt, want] of CASES) {
  test(`refused: ${what}`, () => {
    const p = room();
    breakIt(p);
    const a = check(p);
    assert.equal(a.ok, false, `${what}: the build should be refused`);
    const found = errors(a).find((i) => (!want.code || i.code === want.code) && (!want.scene || i.scene === want.scene)
      && (!want.node || i.node === want.node) && (!want.field || want.field.test(i.field ?? '')));
    assert.ok(found, `${what}: no error ${JSON.stringify(want)} in ${JSON.stringify(a.issues, null, 1)}`);
    assert.ok(found.message && found.fix, 'every error says what is wrong and what is expected');
    if (want.message) assert.match(found.message, want.message);
    if (want.fix) assert.match(found.fix, want.fix);
  });
}

test('--lenient keeps the tolerant validator: the warnings stay warnings, the build rules are not applied', () => {
  const p = room();
  scene(p, 'level').kits['core.light'].brightness = 2;
  scene(p, 'level').kits['core.light'].exposure = -1;
  const a = check(p, { lenient: true });
  assert.equal(a.ok, true, JSON.stringify(a.issues));
  assert.ok(a.issues.some((i) => i.code === 'section_field_unknown' && i.level === 'warning'));
});

test('a warning promoted to an error says so', () => {
  const p = room();
  scene(p, 'level').kits['core.light'].brightness = 2;
  const i = errors(check(p))[0];
  assert.equal(i.promoted, true);
});

test('a scene made only of game.engine content is not empty', () => {
  const p = room();
  p.data.scenes.push({ id: 'arena', instances: [{ id: 'a', archetype: 'x' }] });
  assert.ok(!check(p).issues.some((i) => i.code === 'scene_empty'));
});

test('a bound respected and a maximum crossed', () => {
  const kit = { id: 'test.bounded', version: '1.0.0', expose: [{ id: 'test.bounded.zone', for: ['scene'], at: 'node', props: { size: { type: 'f64', default: 1, min: 1, max: 10 }, depth: { type: 'f64', default: 0, max: 5 } } }] };
  const p = room();
  p.installed.push({ id: 'test.bounded' });
  node(scene(p, 'level'), 'Floor').kits = { 'test.bounded': { size: 11, depth: 2 } };
  const a = call({ op: 'check', project: p, files: {}, kits: [...kits(), kit] });
  const i = errors(a).filter((x) => x.code === 'prop_out_of_range');
  assert.equal(i.length, 1, JSON.stringify(a.issues));
  assert.equal(i[0].node, 'Floor');
  assert.match(i[0].message, /between 1 and 10, 11 is written/);
  node(scene(p, 'level'), 'Floor').kits = { 'test.bounded': { element: 'zone', size: 5, depth: 6 } };
  assert.match(errors(call({ op: 'check', project: p, files: {}, kits: [...kits(), kit] })).find((x) => x.code === 'prop_out_of_range').message, /at most 5/);
});

test('the core answers an unknown op and a request that is not JSON-shaped', () => {
  const a = call({ op: 'fly' });
  assert.equal(a.ok, false);
  assert.equal(a.issues[0].code, 'op_unknown');
  assert.match(call({ op: 'version' }).version, /^\d+\.\d+\.\d+$/);
});

test('the wasm surface: bytes that are not JSON get request_invalid', () => {
  const core = loadCore();
  const bytes = new TextEncoder().encode('not json');
  const ptr = core.gfc_alloc(bytes.length);
  new Uint8Array(core.memory.buffer, ptr, bytes.length).set(bytes);
  const packed = core.gfc_run(ptr, bytes.length);
  const out = JSON.parse(new TextDecoder().decode(new Uint8Array(core.memory.buffer, Number(packed >> 32n), Number(packed & 0xffffffffn))));
  core.gfc_free(Number(packed >> 32n), Number(packed & 0xffffffffn));
  assert.equal(out.issues[0].code, 'request_invalid');
});

test('a missing core says how to get it', () => {
  assert.throws(() => loadCore('/nonexistent/core.wasm', true), /the core of the compiler is missing/);
});
