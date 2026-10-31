#!/usr/bin/env python3
"""Alternate saved before/after native test binaries; run only in-memory benchmarks."""
import argparse
import csv
import pathlib
import re
import statistics
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--diff-before", type=pathlib.Path)
parser.add_argument("--transcript-before", type=pathlib.Path)
parser.add_argument("--search-before", type=pathlib.Path)
parser.add_argument("--gpui-after", type=pathlib.Path)
parser.add_argument("--core-after", type=pathlib.Path)
parser.add_argument("--publication-before", type=pathlib.Path)
parser.add_argument("--pairs", type=int, default=5)
parser.add_argument("--output", type=pathlib.Path, default=root / "docs/performance-runs.tsv")
args = parser.parse_args()
if args.pairs < 1:
    parser.error("--pairs must be positive")

scenarios = [
    ("large-diff", "large_diff_loading_timing", args.diff_before, args.gpui_after),
    ("tool-turn", "long_tool_turn_rebuild_timing", args.transcript_before, args.gpui_after),
    ("filename-ranking", "filename_ranking_timing", args.search_before, args.core_after),
    ("review-publication", "review_publication_timing", args.publication_before, args.gpui_after),
]
scenarios = [scenario for scenario in scenarios if scenario[2] is not None]
if not scenarios or any(scenario[3] is None for scenario in scenarios):
    parser.error("Provide at least one before binary and its corresponding --gpui-after or --core-after binary")
pattern = re.compile(r"(large-diff|tool-turn|filename-ranking|review-publication) trial=(\d+).*?(?:total|elapsed)_ms=([\d.]+)(?: longest_update_ms=([\d.]+))?")
measurements = {}
with args.output.open("w", newline="") as output:
    writer = csv.writer(output, delimiter="\t", lineterminator="\n")
    writer.writerow(["scenario", "side", "pair", "trial", "total_ms", "longest_update_ms"])
    for scenario, test, before, after in scenarios:
        for pair in range(args.pairs):
            for side, binary in [("before", before), ("after", after)]:
                result = subprocess.run([str(binary.resolve()), test, "--ignored", "--nocapture", "--test-threads=1"], capture_output=True, text=True)
                log = result.stdout + result.stderr
                if result.returncode:
                    raise RuntimeError(f"{scenario} {side} failed:\n{log}")
                rows = pattern.findall(log)
                if len(rows) != 5 or any(row[0] != scenario for row in rows):
                    raise RuntimeError(f"Expected five checked trials for {scenario}:\n{log}")
                values = []
                for _, trial, duration, longest in rows:
                    writer.writerow([scenario, side, pair, trial, duration, longest or "NA"])
                    values.append(float(duration))
                output.flush()
                measurements.setdefault((scenario, side), []).extend(values)
                print(f"{scenario} {side} pair={pair} median_ms={statistics.median(values):.3f}", flush=True)
for (scenario, side), values in measurements.items():
    print(f"{scenario} {side} n={len(values)} median_ms={statistics.median(values):.3f} range_ms={min(values):.3f}..{max(values):.3f}")
