set -eu
PACKAGE=@@PACKAGE@@

# A non-interactive login shell may not load version managers that set up
# Node only for interactive shells, so also look where they install it.
find_npx() {
  if command -v npx >/dev/null 2>&1; then
    command -v npx
    return 0
  fi
  for dir in "$HOME/.volta/bin" "$HOME/.local/share/mise/shims" "$HOME/.asdf/shims" \
    "$HOME/.local/bin" "$HOME/.bun/bin" /opt/homebrew/bin /usr/local/bin /usr/bin; do
    if [ -x "$dir/npx" ]; then
      printf '%s\n' "$dir/npx"
      return 0
    fi
  done
  if [ -d "$HOME/.nvm/versions/node" ]; then
    newest=$(ls -1 "$HOME/.nvm/versions/node" | sed 's/^v//' | sort -t. -k1,1n -k2,2n -k3,3n | tail -n 1)
    if [ -n "$newest" ] && [ -x "$HOME/.nvm/versions/node/v$newest/bin/npx" ]; then
      printf '%s\n' "$HOME/.nvm/versions/node/v$newest/bin/npx"
      return 0
    fi
  fi
  return 1
}

if ! NPX=$(find_npx); then
  echo "Node.js was not found for this user. Install Node.js 22.13 or newer on the machine, then try again." >&2
  exit 1
fi
BIN=$(dirname "$NPX")
PATH="$BIN:$PATH"
export PATH
NODE=node
[ -x "$BIN/node" ] && NODE="$BIN/node"
if ! "$NODE" -e 'const [a, b] = process.versions.node.split(".").map(Number); process.exit(a > 22 || (a === 22 && b >= 13) ? 0 : 1)'; then
  echo "MonoCode Host needs Node.js 22.13 or newer; this machine has $("$NODE" --version 2>/dev/null || echo 'an unknown version'). Update Node.js for this user, then try again." >&2
  exit 1
fi
exec "$NPX" --yes --package "$PACKAGE" monocode-host connect --json@@FLAGS@@ </dev/null
