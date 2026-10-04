#!/usr/bin/env python3
"""Compare owned native locale probes with Node Intl, without changing host settings."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

ORACLE = r"""
const pairs = [['_a','.a'],['ä','z'],['é','f'],['e\u0301','é'],['9','10'],['a','B']];
const collator = new Intl.Collator();
const relative = new Intl.RelativeTimeFormat(undefined,{numeric:'auto'});
console.log(JSON.stringify({
  node_version: process.version, icu_version: process.versions.icu,
  collator_locale: collator.resolvedOptions().locale,
  relative_locale: relative.resolvedOptions().locale,
  pairs: pairs.map(([a,b])=>({a,b,order:Math.sign(collator.compare(a,b))})),
  relative: relative.format(-2,'hour')
}));
"""


def run(arguments, env):
    result = subprocess.run(arguments, env=env, capture_output=True, check=True)
    return json.loads(result.stdout.decode("utf-8"))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe", required=True, type=Path)
    parser.add_argument("--node", default="node")
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--linux-matrix", action="store_true")
    args = parser.parse_args()
    rows = []
    configurations = [("host-default", None, None)]
    if args.linux_matrix:
        if not sys.platform.startswith("linux"):
            parser.error("The child environment matrix is only for Linux")
        configurations += [(locale, locale, None) for locale in ["C", "C.UTF-8", "POSIX", "en_US.UTF-8", "fr_FR.UTF-8"]]
        configurations.append(("conflicting-LANGUAGE", "en_US.UTF-8", "fr_FR:de_DE"))
        configurations.append(("conflicting-LANGUAGE-POSIX", "C", "fr_FR"))
    for label, locale, language in configurations:
        env = os.environ.copy()
        if locale:
            for name in ["LANGUAGE", "LC_MESSAGES", "LC_ALL", "LANG"]:
                env.pop(name, None)
            env.update({"LC_ALL": locale, "LANG": locale})
            if language:
                env["LANGUAGE"] = language
        native = run([str(args.probe.resolve())], env)
        node = run([args.node, "--input-type=module", "--eval", ORACLE], env)
        rows.append({
            "case": label,
            "child_locale": locale,
            "child_language": language,
            "native": native,
            "node": node,
            "collation_equal": native["pairs"] == node["pairs"],
            "relative_equal": native["relative"] == node["relative"],
        })
    passed = all(row["collation_equal"] and row["relative_equal"] for row in rows)
    report = {
        "platform": sys.platform,
        "probe_sha256": hashlib.sha256(args.probe.read_bytes()).hexdigest(),
        "passed": passed,
        "cases": rows,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes((json.dumps(report, ensure_ascii=False, indent=2) + "\n").encode("utf-8"))
    print("{} {} default-locale cases".format("PASS" if passed else "FAIL", len(rows)))
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
