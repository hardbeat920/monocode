#!/bin/bash
set -euo pipefail
source_dir=/home/niost/monocode-gpui-qualification
node_bin=$(find "$source_dir/tools/node24" -path '*/bin/node' -type f | sort | tail -n1)
[[ -n "$node_bin" ]]
python3 /mnt/c/Users/niost/locale-system-selection-probe.py \
  --node "$node_bin" \
  --output "$source_dir/artifacts/linux-locale-system-selection.json"
