#!/usr/bin/env bash
# The acceptance of D01 and D03 (gripforgeai/gripforge#420) on the template fps-shooter of the kits repository:
#   1. the template copied into a game (tools/packs copy), its scenes written inline in project.gripforge if it still
#      names scene files (the format of D02), a `$schema` at the head of the project, and the template's bevy/index.rs
#      laid as is at the root of the game (the creator's index.rs);
#   2. gripforge-build (this repository) generates .build/ with the kits of the sources (--sdk, --kits-path);
#   3. the kits of the template and the physics library are built, then the game's module (.build/game);
#   4. tests/play.rs plays the generated game headless through .build/app: magazine, reload, score;
#   5. index.rs and src/ have the same sha256 before and after, index.rs the same as the template's.
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
tpl=$mk/templates/kits/fps-shooter
game=$here/game

echo "== 1. the game: the template copied, index.rs at its root"
rm -rf "$game"
node "$mk/tools/packs/packs.mjs" copy fps-shooter "$game" --engine bevy
node -e '
const fs = require("fs"); const d = process.argv[1];
const p = JSON.parse(fs.readFileSync(d + "/project.gripforge", "utf8"));
let moved = 0;
for (const s of p.data.scenes) {
  if (!s.file) continue;
  const sc = JSON.parse(fs.readFileSync(d + "/" + s.file, "utf8"));
  delete s.file; moved++;
  for (const [k, v] of Object.entries(sc)) if (k !== "$schema") s[k] = v;
}
const out = { "$schema": "./.gripforge/kits/schema/gripforge.full.schema.json", ...p };
fs.writeFileSync(d + "/project.gripforge", JSON.stringify(out, null, 2) + "\n");
console.log(moved ? `scenes written inline (a local migration of ${moved} scene file(s))` : "scenes already inline");
' "$game"
rm -rf "$game/scenes"
cp "$tpl/bevy/index.rs" "$game/index.rs"
sums() { (cd "$game" && find index.rs src project.gripforge -type f | sort | xargs sha256sum); }
before=$(sums)
template=$(sha256sum < "$tpl/bevy/index.rs")

echo "== 2. gripforge-build"
cargo build --manifest-path "$root/Cargo.toml" --bin gripforge-build
"$CARGO_TARGET_DIR/debug/gripforge-build" "$game" --check --sdk "$mk" --kits-path "$mk/kits"
"$CARGO_TARGET_DIR/debug/gripforge-build" "$game" --sdk "$mk" --kits-path "$mk/kits"

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

echo "== 4. the game in a window compiles (.build/app, its bin), then play headless"
cargo build --manifest-path "$game/.build/app/Cargo.toml"
sed "s|@MK@|$mk|" "$here/Cargo.toml.in" > "$here/Cargo.toml"
# The crates the kits repository patches (the [patch.crates-io] of the generated app), at the root of this workspace.
sed -n '/^\[patch.crates-io\]/,/^$/p' "$game/.build/app/Cargo.toml" >> "$here/Cargo.toml"
GF_ACCEPT_TARGET=$CARGO_TARGET_DIR/debug GF_ACCEPT_KITS=$mk/kits \
  cargo test --manifest-path "$here/Cargo.toml" -- --test-threads=1 --nocapture

echo "== 5. the creator's files untouched"
after=$(sums)
[ "$before" = "$after" ] || { echo "the creator's files changed:"; diff <(echo "$before") <(echo "$after"); exit 1; }
[ "$(sha256sum < "$game/index.rs")" = "$template" ] || { echo "index.rs differs from the template's"; exit 1; }
echo "$after"
echo "accept fps-shooter: OK"
