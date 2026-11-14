/**
 * Step 1, reading (lib/project.mjs): the project file of a path, JSON with the line of an error, the scene files an
 * older project still names (sub-scenes included), the kits' catalog (a folder, the package's, none).
 */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { BuildError, findKitsDir, kitCatalog, projectFile, readJson, sceneFiles } from '../lib/project.mjs';
import { KITS, ROOM, ROOM_DIR, projectDir, tmp } from './helpers.mjs';

const code = (fn) => {
  try {
    fn();
  } catch (e) {
    assert.ok(e instanceof BuildError, e.stack);
    return e.issue;
  }
  assert.fail('no BuildError');
};

test('the project file of a file, of a folder with project.gripforge, of a folder with one *.gripforge', () => {
  assert.equal(projectFile(ROOM), ROOM);
  assert.equal(projectFile(ROOM_DIR), ROOM);
  const d = tmp();
  writeFileSync(join(d, 'arena.gripforge'), '{}');
  assert.equal(projectFile(d), join(d, 'arena.gripforge'));
});

test('a missing project, an empty folder, a folder with several projects: project_missing, with the fix', () => {
  const missing = code(() => projectFile(join(tmp(), 'nope.gripforge')));
  assert.equal(missing.code, 'project_missing');
  assert.match(missing.fix, /project\.gripforge/);
  assert.match(code(() => projectFile(tmp())).message, /holds no project\.gripforge/);
  const d = tmp();
  writeFileSync(join(d, 'a.gripforge'), '{}');
  writeFileSync(join(d, 'b.gripforge'), '{}');
  assert.match(code(() => projectFile(d)).message, /several \.gripforge files \(a\.gripforge, b\.gripforge\)/);
});

test('JSON is read; invalid JSON says the file and the line', () => {
  assert.equal(readJson(ROOM).id, 'dev_builder_room');
  const d = projectDir('{\n "id": "x",\n "data": {\n  "scenes": [,]\n }\n}');
  const i = code(() => readJson(join(d, 'project.gripforge')));
  assert.equal(i.code, 'file_not_json');
  assert.equal(i.level, 'error');
  assert.match(i.message, /line 4/);
  assert.match(i.fix, /JSON syntax/);
});

test('the scene files of an older project: by file, through sub-scenes, missing ones left to the core', () => {
  const shelf = { nodes: [{ name: 'Board', type: 'shape' }] };
  const level = { nodes: [{ name: 'Shelf', type: 'scene', scene: 'scenes/shelf.gfscene' }, { name: 'Gone', type: 'scene', scene: 'scenes/gone.gfscene' }] };
  const p = { id: 'old', data: { scenes: [{ id: 'level', file: 'scenes/level.gfscene' }, { id: 'lost', file: 'scenes/lost.gfscene' }] } };
  const d = projectDir(p, { 'scenes/level.gfscene': level, 'scenes/shelf.gfscene': shelf });
  const files = sceneFiles(p, d);
  assert.deepEqual(Object.keys(files).sort(), ['scenes/level.gfscene', 'scenes/shelf.gfscene']);
  assert.deepEqual(files['scenes/shelf.gfscene'], shelf);
});

test('a sub-scene carried in the project is not read from the disk', () => {
  const p = { id: 'one', data: { scenes: [{ id: 'a', nodes: [{ name: 'S', type: 'scene', scene: 'scenes/s.gfscene' }] }], scene_files: { 'scenes/s.gfscene': { nodes: [] } } } };
  assert.deepEqual(sceneFiles(p, projectDir(p, { 'scenes/s.gfscene': '{ not json' })), {});
  assert.deepEqual(sceneFiles({}, tmp()), {});
});

test('a scene file that is not JSON is a build error with its path', () => {
  const p = { id: 'old', data: { scenes: [{ id: 'level', file: 'scenes/level.gfscene' }] } };
  const d = projectDir(p, { 'scenes/level.gfscene': '{ "nodes": [ }' });
  const i = code(() => sceneFiles(p, d));
  assert.equal(i.code, 'file_not_json');
  assert.match(i.file, /level\.gfscene$/);
});

test('the kits folder of a module-kits checkout above a path, none elsewhere', () => {
  assert.equal(findKitsDir(ROOM_DIR), KITS);
  assert.equal(findKitsDir(tmp()), null);
});

test('the catalog: a folder of kits, the package catalog, none', () => {
  const all = kitCatalog(KITS);
  assert.ok(all.length > 10 && all.every((k) => typeof k.id === 'string'));
  const d = tmp();
  mkdirSync(join(d, 'a_kit'));
  writeFileSync(join(d, 'a_kit', 'kit.api.json'), '{"id":"a.kit","version":"1.0.0"}');
  mkdirSync(join(d, 'not_a_kit'));
  assert.deepEqual(kitCatalog(d).map((k) => k.id), ['a.kit']);
  const cat = join(tmp(), 'kits.json');
  writeFileSync(cat, JSON.stringify({ kits: [{ id: 'b.kit' }] }));
  assert.deepEqual(kitCatalog(null, cat).map((k) => k.id), ['b.kit']);
  assert.equal(code(() => kitCatalog(null, join(tmp(), 'none.json'))).code, 'kits_missing');
  assert.equal(code(() => kitCatalog(join(tmp(), 'no-such-dir'))).code, 'kits_missing');
});
