#!/bin/sh
# Measure frames and main-thread CPU while a replayed reply streams, hands-off.
#
#   scripts/perf/frame-run.sh [path/to/monocode-gpui] [seconds]
#   MODE=scroll scripts/perf/frame-run.sh ...   # scroll a finished reply instead
#
# Like stream-profile.sh, but nobody has to click: MONOCODE_PERF_PROMPT makes
# the first chat send a message by itself, and MONOCODE_FRAME_LOG records one
# line per second (frames, draw time, main-thread CPU, which views asked for
# frames). Prints the log and its averages, then closes the window it started.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
app=${1:-"/Applications/Monocode Native.app/Contents/MacOS/monocode-gpui"}
seconds=${2:-20}
replay="$here/fake-claude-stream.py"

[ -x "$app" ] || { echo "No Monocode binary at $app" >&2; exit 1; }

work=$(mktemp -d "${TMPDIR:-/tmp}/monocode-frame-run.XXXXXX")
mkdir -p "$work/data" "$work/project"
log="$work/frames.log"

# MODE=scroll streams a reply first, then measures while the finished
# transcript scrolls up and down at SCROLL_SPEED pixels per second.
mode=${MODE:-stream}
if [ "$mode" = scroll ]; then
    export REPLAY_SECONDS=${SCROLL_REPLY_SECONDS:-25} MONOCODE_PERF_SCROLL=${SCROLL_SPEED:-1500}
else
    export REPLAY_SECONDS=$((seconds + 15))
fi
export REPLAY_MARK="$work/streaming"
MONOCODE_NATIVE_DATA_DIR="$work/data" MONOCODE_CLAUDE_BINARY="$replay" \
    MONOCODE_PERF_PROMPT="stream" MONOCODE_FRAME_LOG="$log" \
    "$app" --dev-data --cwd "$work/project" >"$work/app.log" 2>&1 &
pid=$!
trap 'kill "$pid" 2>/dev/null || true' EXIT INT TERM

waited=0
until [ -e "$work/streaming" ]; do
    kill -0 "$pid" 2>/dev/null || { echo "Monocode exited; see $work/app.log" >&2; exit 1; }
    waited=$((waited + 1))
    [ "$waited" -lt 60 ] || { echo "No reply streamed in 60s; see $work/app.log" >&2; exit 1; }
    sleep 1
done
if [ "$mode" = scroll ]; then
    # Let the reply finish and the scroll get going.
    sleep $((REPLAY_SECONDS + 4))
else
    sleep "${PROFILE_DELAY:-3}"
fi
: >"$log"
# PROFILE=1 also samples stacks over the same window with Monocode Performance.
profiler=${MONOCODE_PROFILER:-"$HOME/code/mono-perf-chip/dist/Monocode Performance.app/Contents/MacOS/MonocodePerformance"}
if [ "${PROFILE:-0}" = 1 ] && [ -x "$profiler" ]; then
    mkdir -p "$work/profile"
    "$profiler" --profile "$seconds" --pid "$pid" --output "$work/profile" >"$work/profile/summary.json"
else
    sleep "$seconds"
fi
cp "$log" "$work/window.log"

cat "$work/window.log"
awk '
    { for (i = 1; i <= NF; i++) { split($i, kv, "="); v[kv[1]] = kv[2] }
      n++; fps += v["fps"]; layout += v["layout"]; paint += v["paint"]
      busy += v["busy"] + 0; main += v["main"] + 0
      split(v["draw"], d, "/"); draw += d[1] + 0 }
    END { if (n) printf "\nAVERAGE over %ds: fps=%.0f layout=%.0f paint=%.0f draw=%.2fms busy=%.1f%% main=%.1f%%\n", \
        n, fps/n, layout/n, paint/n, draw/n, busy/n, main/n }
' "$work/window.log"
echo "Log: $work/window.log"
[ -d "$work/profile" ] && echo "Profile: $(ls -d "$work"/profile/profile-* 2>/dev/null)"
exit 0
