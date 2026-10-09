#!/usr/bin/env bash
set -euo pipefail
VERSION="${1:?Provide the desktop release version}"
PACKAGE="monocode-host@$VERSION"
REGISTRY="https://registry.npmjs.org"
if [[ "$(npm view "$PACKAGE" version --registry "$REGISTRY" 2>/dev/null || true)" != "$VERSION" ]]; then
  if [[ -z "${NPM_TOKEN:-}" ]]; then
    echo "$PACKAGE must be published before the desktop release. Configure NPM_TOKEN with publish access to monocode-host." >&2
    exit 1
  fi
  shopt -s nullglob
  HOST_NPM=(build/host-npm-package/monocode-host-*.tgz)
  if (( ${#HOST_NPM[@]} != 1 )); then
    echo "Expected one host npm package" >&2
    exit 1
  fi
  printf '//registry.npmjs.org/:_authToken=%s\n' "$NPM_TOKEN" > "$RUNNER_TEMP/.npmrc"
  NPM_CONFIG_USERCONFIG="$RUNNER_TEMP/.npmrc" npm publish "${HOST_NPM[0]}" --access public --registry "$REGISTRY"
fi
# Verify public availability, including after a resumed release.
for attempt in {1..12}; do
  if [[ "$(npm view "$PACKAGE" version --registry "$REGISTRY" 2>/dev/null || true)" == "$VERSION" ]]; then
    echo "$PACKAGE is available on npm"
    exit 0
  fi
  sleep 5
done
echo "$PACKAGE is not publicly available on npm. Desktop publication is blocked." >&2
exit 1
