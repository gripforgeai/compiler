#!/usr/bin/env bash
# The acceptance of the game's code declared by the project (gripforgeai/gripforge#420, `sources` of project.gripforge)
# on the template fps-shooter of the kits repository:
#   1. the template copied into a game (tools/packs copy), its scenes inline in project.gripforge (the format of
#      module-kits#1802), a `$schema` at its head, and `"sources": ["rules.rs"]` with the example of this repository
#      (tests/game-code/rules.rs) as the game's code; the template's Bevy index.rs is NOT declared: it is not compiled
#      (no file is taken by convention: the build warns source_undeclared);
#   2. gripforge-build (this repository) generates .build/ with the kits of the sources (--sdk, --kits-path);
#   3. the kits of the template and the physics library are built, then the game's module (.build/game) with rules.rs;
#   4. tests/play.rs plays the game headless through the Bevy gateway, game.main loaded beside the kits: the setup of
#      the game's code ran before the first scene, the scene was entered, the code runs at the tick;
#   5. rules.rs and project.gripforge have the same sha256 before and after.
#
#   tests/accept/fps-shooter/run.sh <module-kits checkout>        (CARGO_TARGET_DIR: default <checkout>/target)
#
# Heavy (Bevy, the kits): run it on a build machine, not in the CI.
set -euo pipefail
[ $# -eq 1 ] || { echo "usage: $0 <module-kits checkout>" >&2; exit 2; }
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)
mk=$(cd "$1" && pwd)
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$mk/target}
game=$here/game

echo "== 1. the game: the template copied, its code declared in sources"
rm -rf "$game"
node "$mk/tools/packs/packs.mjs" copy fps-shooter "$game" --engine bevy
node -e '
const fs = require("fs"); const d = process.argv[1];
const p = JSON.parse(fs.readFileSync(d + "/project.gripforge", "utf8"));
if (p.data.scenes.some((s) => s.file)) throw new Error("a scene in a file: the template is not in the format of module-kits#1802");
const out = { "$schema": "./.gripforge/kits/schema/gripforge.full.schema.json", sources: ["rules.rs"], ...p };
fs.writeFileSync(d + "/project.gripforge", JSON.stringify(out, null, 2) + "\n");
' "$game"
cp "$root/tests/game-code/rules.rs" "$game/rules.rs"
sums() { (cd "$game" && sha256sum rules.rs project.gripforge); }
before=$(sums)

echo "== 2. gripforge-build"
cargo build --manifest-path "$root/Cargo.toml" --bin gripforge-build
"$CARGO_TARGET_DIR/debug/gripforge-build" "$game" --check --sdk "$mk" --kits-path "$mk/kits"
"$CARGO_TARGET_DIR/debug/gripforge-build" "$game" --sdk "$mk" --kits-path "$mk/kits" --json > "$here/build.json"
node -e '
const r = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
const a = r.artifacts || {};
if (a.gameModule !== "crate::rules::GameRules") throw new Error("the module of the game: " + a.gameModule);
if (r.issues.some((i) => i.code === "source_undeclared" && /index\.rs/.test(i.message))) console.log("index.rs of the template: not compiled (warning source_undeclared)");
' "$here/build.json"
grep -q 'pub mod rules;' "$game/.build/game/src/lib.rs"

echo "== 3. the kits of the template, the physics library, the game's module"
crates=$(cd "$mk" && node -e '
const { execFileSync } = require("child_process"); const fs = require("fs"); const path = require("path");
const meta = JSON.parse(execFileSync("cargo", ["metadata", "--no-deps", "--format-version", "1"], { encoding: "utf8", maxBuffer: 1 << 30 }));
const byDir = new Map(meta.packages.map((p) => [path.relative(meta.workspace_root, path.dirname(p.manifest_path)), p.name]));
const t = JSON.parse(fs.readFileSync("templates/kits/fps-shooter/template.json", "utf8"));
const out = t.kits.map((k) => byDir.get(`kits/${k.id.replace(/[.-]/g, "_")}/logic`) ?? (() => { throw new Error(`no crate for ${k.id}`); })());
out.push("gfkit-core-physics-module");
console.log(out.map((c) => `-p ${c}`).join(" "));
')
(cd "$mk" && cargo build $crates)
cargo build --manifest-path "$game/.build/game/Cargo.toml"

echo "== 4. play headless"
sed "s|@MK@|$mk|" "$here/Cargo.toml.in" > "$here/Cargo.toml"
# The crates the kits repository patches, as it builds the gateway (its [patch.crates-io], paths made absolute).
node -e '
const fs = require("fs"); const path = require("path"); const mk = process.argv[1];
const lines = fs.readFileSync(path.join(mk, "Cargo.toml"), "utf8").split("\n");
let inside = false; const out = ["", "[patch.crates-io]"];
for (const l of lines) {
  const t = l.trim();
  if (t.startsWith("[")) { inside = t === "[patch.crates-io]"; continue; }
  const m = inside && t.match(/^([A-Za-z0-9_-]+)\s*=\s*\{\s*path\s*=\s*"([^"]+)"/);
  if (m) out.push(`${m[1]} = { path = ${JSON.stringify(path.join(mk, m[2]))} }`);
}
fs.appendFileSync(process.argv[2], out.join("\n") + "\n");
' "$mk" "$here/Cargo.toml"
GF_ACCEPT_TARGET=$CARGO_TARGET_DIR/debug GF_ACCEPT_KITS=$mk/kits \
  cargo test --manifest-path "$here/Cargo.toml" -- --test-threads=1 --nocapture

echo "== 5. the creator's files untouched"
after=$(sums)
[ "$before" = "$after" ] || { echo "the creator's files changed:"; diff <(echo "$before") <(echo "$after"); exit 1; }
echo "$after"
echo "accept fps-shooter: OK"
