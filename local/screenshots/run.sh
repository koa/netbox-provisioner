#!/usr/bin/env bash
# Runs screenshot.mjs with the playwright browsers from nixpkgs. The npm package must be the
# driver's version exactly (the browsers' revisions belong to it); when nixpkgs has moved on
# from package.json, that version is installed here without saving it.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
nix=(nix --extra-experimental-features 'nix-command flakes')
version="$("${nix[@]}" eval --raw nixpkgs#playwright-driver.version)"
export PLAYWRIGHT_BROWSERS_PATH="$("${nix[@]}" build --no-link --print-out-paths nixpkgs#playwright-driver.browsers)"
export PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS=true
export PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1
cd "$here"
[ -d node_modules ] || npm install --no-audit --no-fund
installed="$(node -p "require('./node_modules/playwright/package.json').version" 2>/dev/null || true)"
if [ "$installed" != "$version" ]; then
  echo "playwright $version for the nixpkgs browsers (installed: ${installed:-none})" >&2
  npm install --no-audit --no-fund --no-save "playwright@$version"
fi
exec node "$here/screenshot.mjs" "$@"
