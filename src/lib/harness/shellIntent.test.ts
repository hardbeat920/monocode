import { describe, expect, it } from "vitest";
import {
  formatShellIntent,
  inferShellIntent,
  unwrapShellCommand,
} from "./shellIntent";

describe("inferShellIntent", () => {
  it("reads a file from cat / head / sed -n", () => {
    expect(inferShellIntent("cat package.json")).toEqual({
      verb: "Read",
      path: "package.json",
    });
    expect(
      inferShellIntent(
        'cd /Users/nikolaypetkov/code/agent-terminal && ls && echo "---" && cat package.json | head -60',
      ),
    ).toEqual({ verb: "Read", path: "package.json" });
    expect(
      inferShellIntent(
        "ls src/lib/harness/ && echo \"=== preview ===\" && sed -n '1,140p' src/lib/harness/preview.ts",
      ),
    ).toEqual({
      verb: "Read",
      path: "src/lib/harness/preview.ts",
      startLine: 1,
    });
    expect(
      inferShellIntent("sed -n '713,1200p' src/surfaces/AgentTranscript.tsx"),
    ).toEqual({
      verb: "Read",
      path: "src/surfaces/AgentTranscript.tsx",
      startLine: 713,
    });
    expect(
      inferShellIntent(
        `/bin/zsh -lc "nl -ba src/lib/orchestration.ts | sed -n '1,260p'"`,
      ),
    ).toEqual({
      verb: "Read",
      path: "src/lib/orchestration.ts",
    });
    expect(
      inferShellIntent(
        `/bin/zsh -lc "sed -n '1,260p' src/surfaces/transcriptActivity.ts
sed -n '880,980p' src/surfaces/transcriptActivity.test.ts
sed -n '960,1060p' src/index.css"`,
      ),
    ).toEqual({
      verb: "Read",
      path: "src/index.css",
      startLine: 960,
    });
  });

  it("treats grep / rg as Find", () => {
    expect(
      inferShellIntent(
        'grep -n "^function \\|^const .* = memo" src/surfaces/AgentTranscript.tsx',
      ),
    ).toEqual({
      verb: "Find",
      query: "^function |^const .* = memo",
      path: "src/surfaces/AgentTranscript.tsx",
    });
    expect(inferShellIntent("rg -n isReadTool src/lib/harness")).toEqual({
      verb: "Find",
      query: "isReadTool",
      path: "src/lib/harness",
    });
    expect(
      inferShellIntent(
        `/bin/bash -lc "rg -n 'submissionError|hydrate' src/lib"`,
      ),
    ).toEqual({
      verb: "Find",
      query: "submissionError|hydrate",
      path: "src/lib",
    });
    expect(
      inferShellIntent(
        `/bin/zsh -lc "rg -n \\\"function upsertTool\\\" src/lib/harness/apply.ts && sed -n '520,620p' src/lib/harness/apply.ts
rg -n \\\"mapApprovalRequest\\\" src/lib/harness/codexProtocol.test.ts | head -n 60
sed -n '560,640p' src/lib/harness/codexProtocol.test.ts"`,
      ),
    ).toEqual({
      verb: "Read",
      path: "src/lib/harness/codexProtocol.test.ts",
      startLine: 560,
    });
    expect(inferShellIntent("find src -name '*.ts'")).toEqual({
      verb: "Find",
      query: "*.ts",
      path: "src",
    });
  });

  it("lists a directory when that is all the command does", () => {
    expect(inferShellIntent("ls src/lib/harness/")).toEqual({
      verb: "List",
      path: "src/lib/harness",
    });
    expect(inferShellIntent("ls -la")).toBeUndefined();
    expect(inferShellIntent("rg --files | sed -n '1,240p'")).toEqual({
      verb: "Find",
      query: "files",
    });
  });

  it("leaves real shell as the command", () => {
    expect(inferShellIntent("git status -s")).toBeUndefined();
    expect(inferShellIntent("/bin/zsh -lc 'git status -s'")).toBeUndefined();
    expect(inferShellIntent("git diff src/surfaces/AgentTranscript.tsx")).toBeUndefined();
    expect(inferShellIntent("npm test")).toBeUndefined();
    expect(inferShellIntent("cat file && python script.py")).toBeUndefined();
    expect(inferShellIntent("cat $(echo foo)")).toBeUndefined();
    expect(inferShellIntent("python3 - <<'PY'")).toBeUndefined();
    expect(inferShellIntent("rm src/hooks/useActivityTicker.ts && python3 - <<'PY'")).toBeUndefined();
  });

  it("treats file-mutating bash as Edit / Write", () => {
    expect(inferShellIntent("sed -i 's/a/b/' src/app.ts")).toEqual({
      verb: "Edit",
      path: "src/app.ts",
    });
    expect(
      inferShellIntent(
        "sed -i '' 's/zen-ticker-live/zen-tool-spin/' src/surfaces/AgentTranscript.tsx",
      ),
    ).toEqual({
      verb: "Edit",
      path: "src/surfaces/AgentTranscript.tsx",
    });
    expect(
      inferShellIntent("cat >> src/surfaces/transcriptActivity.ts <<'TS'"),
    ).toEqual({
      verb: "Edit",
      path: "src/surfaces/transcriptActivity.ts",
    });
    expect(inferShellIntent("cat package.json > out.json")).toEqual({
      verb: "Write",
      path: "out.json",
    });
    expect(inferShellIntent("tee src/index.css")).toEqual({
      verb: "Write",
      path: "src/index.css",
    });
  });

  it("does not hang on fd redirects like 2>&1", () => {
    expect(inferShellIntent("grep foo src/app.ts 2>&1")).toEqual({
      verb: "Find",
      query: "foo",
      path: "src/app.ts",
    });
    expect(inferShellIntent("cat package.json 2>&1")).toEqual({
      verb: "Read",
      path: "package.json",
    });
    expect(inferShellIntent("git status 2>&1")).toBeUndefined();
    expect(inferShellIntent("grep foo src/app.ts 2>/dev/null")).toEqual({
      verb: "Find",
      query: "foo",
      path: "src/app.ts",
    });
  });

  it("skips already-labelled rows and huge scripts", () => {
    expect(inferShellIntent("Read src/lib/appearance.ts")).toBeUndefined();
    expect(inferShellIntent(`cat ${"a".repeat(3000)}.ts`)).toBeUndefined();
  });
});

describe("formatShellIntent", () => {
  it("prefers a display path passed in from the transcript", () => {
    expect(
      formatShellIntent(
        { verb: "Read", path: "/Users/me/proj/src/app.ts" },
        "src/app.ts",
      ),
    ).toBe("Read src/app.ts");
  });
});

describe("unwrapShellCommand", () => {
  it("unwraps POSIX shells without including trailing shell arguments", () => {
    expect(unwrapShellCommand(`/bin/zsh -lc "npm test -- --run app.test.ts"`)).toBe(
      "npm test -- --run app.test.ts",
    );
    expect(unwrapShellCommand(`/bin/zsh -lc "rg -n \\"foo\\" src" ignored`)).toBe(
      'rg -n "foo" src',
    );
  });

  it("unwraps PowerShell command remainders", () => {
    expect(
      unwrapShellCommand(
        `"C:\\Program Files\\PowerShell\\7\\pwsh.exe" -NoLogo -NoProfile -Command 'rg -n foo src'`,
      ),
    ).toBe("rg -n foo src");
    expect(
      unwrapShellCommand(
        "powershell.exe -ExecutionPolicy Bypass -Command Get-Content package.json",
      ),
    ).toBe("Get-Content package.json");
    expect(unwrapShellCommand("pwsh -c Get-Content package.json")).toBe(
      "Get-Content package.json",
    );
    expect(
      unwrapShellCommand(`pwsh "-Command" "Get-Content package.json"`),
    ).toBe("Get-Content package.json");
    expect(
      unwrapShellCommand(`pwsh -Command 'Get-Date' '-Format' 'yyyy-MM-dd'`),
    ).toBe(`'Get-Date' '-Format' 'yyyy-MM-dd'`);
  });

  it("unwraps cmd command remainders", () => {
    expect(unwrapShellCommand(`cmd.exe /d /s /c "npm test"`)).toBe("npm test");
  });

  it("stops scanning PowerShell launcher options at -File", () => {
    expect(unwrapShellCommand(`pwsh -File script.ps1 -Mode -Command build`)).toBe(
      `pwsh -File script.ps1 -Mode -Command build`,
    );
    expect(unwrapShellCommand(`pwsh -f script.ps1 -Mode -c build`)).toBe(
      `pwsh -f script.ps1 -Mode -c build`,
    );
    expect(
      unwrapShellCommand(`pwsh "-File" script.ps1 "-Command" build`),
    ).toBe(`pwsh "-File" script.ps1 "-Command" build`);
  });

  it("leaves ordinary and incomplete commands unchanged", () => {
    expect(unwrapShellCommand("git status --short")).toBe("git status --short");
    expect(unwrapShellCommand(`pwsh -Command 'npm test`)).toBe(
      `pwsh -Command 'npm test`,
    );
  });
});
