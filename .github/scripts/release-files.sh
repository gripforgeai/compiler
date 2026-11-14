#!/usr/bin/env bash
# Writes SHA256SUMS, release.json and notes.md of a release from the archives in <dir>.
# Usage: release-files.sh <version> <dir>   (env: REPO, TAG, COMMIT, DATE)
# The archives are named gripforge-build-<version>-<target>.(tar.gz|zip); one release.json entry per platform.
set -euo pipefail
v=$1; dir=$2
: "${REPO:?}" "${TAG:?}" "${COMMIT:?}" "${DATE:?}"
base="https://github.com/$REPO/releases/download/$TAG"
cd "$dir"
files=$(ls gripforge-build-"$v"-*.tar.gz gripforge-build-"$v"-*.zip 2>/dev/null | sort)
[ -n "$files" ] || { echo "no archive in $dir" >&2; exit 1; }
: > SHA256SUMS
platforms='[]'
for f in $files; do
  sum=$(sha256sum "$f" | cut -d' ' -f1)
  echo "$sum  $f" >> SHA256SUMS
  target=${f#gripforge-build-"$v"-}; target=${target%.tar.gz}; target=${target%.zip}
  case $target in
    x86_64-unknown-linux-gnu) os=linux; arch=x64 ;;
    aarch64-unknown-linux-gnu) os=linux; arch=arm64 ;;
    x86_64-pc-windows-msvc) os=windows; arch=x64 ;;
    x86_64-apple-darwin) os=macos; arch=x64 ;;
    aarch64-apple-darwin) os=macos; arch=arm64 ;;
    *) echo "unknown target $target" >&2; exit 1 ;;
  esac
  platforms=$(jq -c --arg name "$f" --arg target "$target" --arg os "$os" --arg arch "$arch" \
    --arg url "$base/$f" --arg sha "$sum" --argjson size "$(wc -c < "$f" | tr -d ' ')" \
    '. + [{platform: ($os + "-" + $arch), os: $os, arch: $arch, target: $target, name: $name, url: $url, size: $size, sha256: $sha}]' <<<"$platforms")
done
jq -n --arg v "$v" --arg tag "$TAG" --arg date "$DATE" --arg commit "$COMMIT" --arg repo "$REPO" --argjson platforms "$platforms" '{
  schema: "gripforge.compiler-release/2",
  version: $v, tag: $tag, date: $date, commit: $commit,
  source: ("https://github.com/" + $repo + "/tree/" + $tag),
  binary: "gripforge-build",
  platforms: $platforms
}' > release.json
# SHA256SUMS also covers release.json.
echo "$(sha256sum release.json | cut -d' ' -f1)  release.json" >> SHA256SUMS
# Notes: the annotated tag's message, else the CHANGELOG section of the version.
{
  if [ -n "${TAG_MESSAGE:-}" ]; then printf '%s\n' "$TAG_MESSAGE"
  elif [ -f "$OLDPWD/CHANGELOG.md" ]; then
    awk -v v="$v" '/^## /{p = (index($0, "[" v "]") > 0); next} p' "$OLDPWD/CHANGELOG.md"
  fi
  printf '\n### Install\n\nDownload the archive of your platform below, check it against `SHA256SUMS`, unpack it: `gripforge-build` is one binary, no runtime. `release.json` lists the files.\n\n```\n'
  cat SHA256SUMS
  printf '```\n'
} > notes.md
