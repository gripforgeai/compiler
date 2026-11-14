/**
 * Step 7, the messages (lib/report.mjs): where an issue is in words (file › scene › element › property), one line with
 * what is expected, the JSON report gripforge.build-report/1.
 */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { line, report, where } from '../lib/report.mjs';

test('where: the project for an inline scene, the file of an older one, each part only when known', () => {
  assert.equal(where({ file: '#level', node: 'Hall/Door', field: 'kits.core.light.x' }), 'project.gripforge › scene level › element Hall/Door › property kits.core.light.x');
  assert.equal(where({ file: 'scenes/level.gfscene', scene: 'level' }, 'arena.gripforge'), 'scenes/level.gfscene › scene level');
  assert.equal(where({ field: 'installed' }, 'arena.gripforge'), 'arena.gripforge › property installed');
  assert.equal(where({}), 'project.gripforge');
});

test('a line: level, code, where, the message, the expected; a promoted warning says why it stops the build', () => {
  const i = { level: 'error', code: 'prop_invalid', file: '#level', node: 'Sun', field: 'light.intensity', message: 'light.intensity is not a number', fix: 'write light.intensity as a number', promoted: true };
  assert.equal(line(i), 'error prop_invalid — project.gripforge › scene level › element Sun › property light.intensity: light.intensity is not a number (a warning of the tolerant validator: a build accepts no guess). Expected: write light.intensity as a number');
  assert.equal(line({ level: 'warning', code: 'sound_not_placed', message: 'read, not played.' }), 'warning sound_not_placed — project.gripforge: read, not played.');
});

test('the JSON report counts and locates every issue', () => {
  const r = report({ ok: false, projectName: 'p.gripforge', issues: [{ level: 'error', code: 'a', message: 'm', fix: 'f', file: '#s' }, { level: 'warning', code: 'b', message: 'm', fix: 'f' }] });
  assert.equal(r.schema, 'gripforge.build-report/1');
  assert.equal(r.errors, 1);
  assert.equal(r.warnings, 1);
  assert.equal(r.issues[0].where, 'p.gripforge › scene s');
  assert.deepEqual(r.steps, []);
  assert.deepEqual(r.artifacts, {});
  assert.equal(r.out, null);
});
