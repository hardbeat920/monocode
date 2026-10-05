#!/bin/sh
# Run only after the source freeze and explicit Mac Cargo transfer.
set -eu
if [ "$#" -ne 2 ]; then
    printf 'Usage: run-final-macos.sh MANIFEST UNIQUE_OUTPUT_DIRECTORY\n' >&2
    exit 2
fi
if [ "$(uname -s)" != Darwin ]; then
    printf 'This wrapper requires macOS.\n' >&2
    exit 2
fi
TASK_BUILD_ROOT="$HOME/Library/Caches/monocode-gpui-build"
source_dir="$TASK_BUILD_ROOT/source"
manifest="$1"
output_dir="$2"
if [ ! -f "$manifest" ]; then
    printf 'The frozen source manifest does not exist.\n' >&2
    exit 2
fi
if [ -e "$output_dir" ]; then
    printf 'Choose a new output directory to preserve previous qualification evidence.\n' >&2
    exit 2
fi
export PATH="$TASK_BUILD_ROOT/toolchain/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"
export CARGO_HOME="$TASK_BUILD_ROOT/cargo"
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export DYLD_LIBRARY_PATH="$TASK_BUILD_ROOT/toolchain/lib"
mkdir -p "$output_dir"
cp "$manifest" "$output_dir/source-manifest.json"
python3 "$TASK_BUILD_ROOT/final-native-check.py" \
    --source "$source_dir" \
    --output "$output_dir" \
    --manifest "$output_dir/source-manifest.json" \
    --manifest-helper "$TASK_BUILD_ROOT/final-source-manifest.py" \
    --require-localization \
    --require-javascript-search
