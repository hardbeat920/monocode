#!/usr/bin/env bash
# Real Linux CoW coverage on a disposable loopback filesystem, never a host disk.
set -euo pipefail
cd "$(dirname "$0")/.."
if [ "$(uname -s)" != Linux ]; then
  echo 'Btrfs integration tests require Linux.' >&2
  exit 1
fi
for tool in btrfs mkfs.btrfs mount umount mountpoint truncate cargo; do
  command -v "$tool" >/dev/null || { echo "Missing test dependency: $tool" >&2; exit 1; }
done
btrfs_sudo=()
if [ "$(id -u)" -ne 0 ]; then btrfs_sudo=(sudo); fi
btrfs_work=$(mktemp -d "${TMPDIR:-/tmp}/monocode-btrfs.XXXXXX")
btrfs_mount="$btrfs_work/fs"
cleanup() {
  local btrfs_exit_status=$?
  trap - EXIT
  if mountpoint -q "$btrfs_mount"; then
    "${btrfs_sudo[@]}" umount "$btrfs_mount" || {
      echo "Test mount retained at $btrfs_mount: unmount it before removing $btrfs_work" >&2
      exit 1
    }
  fi
  rm -rf -- "$btrfs_work"
  exit "$btrfs_exit_status"
}
trap cleanup EXIT
mkdir "$btrfs_mount"
truncate -s 2G "$btrfs_work/test.img"
"${btrfs_sudo[@]}" mkfs.btrfs -f "$btrfs_work/test.img"
"${btrfs_sudo[@]}" mount -o loop,user_subvol_rm_allowed "$btrfs_work/test.img" "$btrfs_mount"
"${btrfs_sudo[@]}" chown "$(id -u):$(id -g)" "$btrfs_mount"
mkdir "$btrfs_mount/tmp"
export TMPDIR="$btrfs_mount/tmp"
export MONOCODE_REQUIRE_COW=1
cargo clippy --locked -p monocode-isolation --all-targets -- -D warnings
cargo test --locked -p monocode-isolation
cargo test --locked --manifest-path crates/monocode-gpui/Cargo.toml -p monocode-core --profile native-dev isolation::
