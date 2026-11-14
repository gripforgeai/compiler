#!/usr/bin/env node
/**
 * gripforge-build — the command behind `gripforge project build` (npm @gripforge/compiler). The command line is
 * lib/cli.mjs (usage there, and in README.md); this file only runs it.
 */
import { main } from '../lib/cli.mjs';

process.exit(await main(process.argv.slice(2)));
