#!/usr/bin/env python3
"""Run one explicit OpenCode 2 smoke test without inherited provider credentials."""

import argparse
import os
import subprocess


def credential_name(name):
    return (
        name.startswith("OPENCODE_")
        or name.endswith("_API_KEY")
        or name.endswith("_AUTH_TOKEN")
        or name in {
            "ANTHROPIC_AUTH_TOKEN", "OPENAI_ACCESS_TOKEN", "GOOGLE_APPLICATION_CREDENTIALS",
            "AI_GATEWAY_API_KEY", "FX_AI_GATEWAY_API_KEY", "VERCEL_OIDC_TOKEN",
            "AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY", "AWS_SESSION_TOKEN", "AWS_PROFILE",
            "AZURE_CLIENT_SECRET", "GITHUB_TOKEN", "GH_TOKEN",
        }
    )


parser = argparse.ArgumentParser()
parser.add_argument("--test-binary", required=True)
parser.add_argument("--cli", required=True)
args = parser.parse_args()
environment = {name: os.environ[name] for name in os.environ if not credential_name(name)}
environment["MONOCODE_OPENCODE_V2_BIN"] = os.path.abspath(args.cli)
result = subprocess.run(
    [args.test_binary,
     "providers::opencode::v2::transport_tests::isolated_live_free_model_turn",
     "--exact", "--ignored", "--nocapture", "--test-threads=1"],
    env=environment,
    timeout=180,
    check=False,
)
raise SystemExit(result.returncode)
