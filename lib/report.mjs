/**
 * The issues of a build, said so that an AI corrects the project alone: where (file › scene › element › property),
 * what is wrong, what is expected. One line each in a terminal, or the JSON report (`--json`,
 * `gripforge.build-report/1`) the MCP and the CLI read. The codes are the core's (docs/ecrire-un-projet.md) plus
 * those of the build (README.md of the package).
 *
 * Used by bin/gripforge-build.mjs and lib/build.mjs.
 */

/** Where an issue is, in words: `project.gripforge › scene level › element Hall/Door › property kits.core.light.x`. */
export function where(i, projectName = 'project.gripforge') {
  const parts = [];
  const file = i.file && !i.file.startsWith('#') ? i.file : projectName;
  parts.push(file);
  const scene = i.scene ?? (i.file?.startsWith('#') ? i.file.slice(1) : null);
  if (scene) parts.push(`scene ${scene}`);
  if (i.node) parts.push(`element ${i.node}`);
  if (i.field) parts.push(`property ${i.field}`);
  return parts.join(' › ');
}

/** One line: `error <code> — <where>: <message>. Expected: <fix>`. */
export function line(i, projectName) {
  const fix = i.fix ? ` Expected: ${i.fix}` : '';
  const promoted = i.promoted ? ' (a warning of the tolerant validator: a build accepts no guess)' : '';
  const said = `${i.message}${promoted}`;
  return `${i.level} ${i.code} — ${where(i, projectName)}: ${said}${said.endsWith('.') ? '' : '.'}${fix}`;
}

/** The JSON report of a build. */
export function report(result) {
  return {
    schema: 'gripforge.build-report/1',
    ok: result.ok,
    project: result.project ?? null,
    out: result.out ?? null,
    errors: result.issues.filter((i) => i.level === 'error').length,
    warnings: result.issues.filter((i) => i.level !== 'error').length,
    issues: result.issues.map((i) => ({ ...i, where: where(i, result.projectName) })),
    steps: result.steps ?? [],
    artifacts: result.artifacts ?? {},
  };
}
