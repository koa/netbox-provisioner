#!/usr/bin/env bash
# Runs screenshot.mjs with the playwright browsers from nixpkgs (same version as package.json).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
nix=(nix --extra-experimental-features 'nix-command flakes')
export PLAYWRIGHT_BROWSERS_PATH="$("${nix[@]}" build --no-link --print-out-paths nixpkgs#playwright-driver.browsers)"
export PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS=true
[ -d "$here/node_modules" ] || (cd "$here" && PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1 npm install --no-audit --no-fund)
exec node "$here/screenshot.mjs" "$@"
