import { useState } from "react";
import { createRoot } from "react-dom/client";
import {
  AgentTranscript,
  ToolCall,
} from "../../src/features/sessions/ui/AgentTranscript";
import {
  newSession,
  type Block,
} from "../../src/features/sessions/model/session";
import {
  appendSteerUser,
  appendUser,
  applyHarnessEvent,
  stopStreaming,
} from "../../src/integrations/harness/core/apply";
import { sanitizeSessionForPersist } from "../../src/features/sessions/data/sessionStore";
import { saveTranscriptAnchor } from "../../src/features/settings/model/appearance";
import "../../src/styles/index.css";

const params = new URLSearchParams(location.search);
if (params.has("scroll"))
  document.getElementById("root")!.style.height = "140px";
saveTranscriptAnchor(false);
localStorage.setItem("monocode.pacedReveal", params.has("paced") ? "1" : "0");
document.documentElement.classList.toggle("theme-light", params.has("light"));
let initial = newSession("codex", "/fixture");
if (params.has("history"))
  initial.blocks = Array.from({ length: 12 }, (_, i): Block[] => [
    { id: `u-${i}`, role: "user", text: `Earlier request ${i}` },
    {
      id: `a-${i}`,
      role: "assistant",
      text: "Earlier settled paragraph. ".repeat(50),
    },
  ]).flat();
initial = appendUser(initial, "Plan the integration");
initial = applyHarnessEvent(initial, {
  type: "message.delta",
  text: "1. integration",
});
if (params.has("results")) {
  initial = appendUser(
    newSession("codex", "/fixture"),
    "Inspect the tool results",
  );
  const outputs = params.has("opencode")
    ? [
        [
          "opencode-find",
          "Find TODO|FIXME|HACK|XXX",
          "Found 47 matches\nD:\\primewire-desktop\\inspection\\unblock-filters.txt:\n  Line 7937: fresh-babes.com###XXXGirls\n\nD:\\primewire-desktop\\outreach\\primewire-email-draft.eml:\n  Line 316: abcdefg\n  Line 2892: xyz123",
        ],
        [
          "opencode-read",
          "Read src/main.rs",
          "use std::sync::Arc;\n\nfn main() {\n    println!(\"hello\");\n}",
        ],
        ["opencode-shell", "git status", "fatal: not a git repository"],
      ]
    : [
    [
      "json",
      "Run structured result",
      params.has("table-shell")
        ? JSON.stringify({
            commandLine: 'Get-ChildItem -Path "D:\\primewire-desktop"',
            workingDir: "D:\\primewire-desktop",
            exitCode: 0,
            combinedOutput:
              "Directory: D:\\primewire-desktop\n\nMode      LastWriteTime          Length Name\n----      -------------          ------ ----\nd----     10/16/2026  8:31 PM            assets\nd----     10/16/2026  8:36 PM            inspection\n",
          })
        : params.has("capped-shell")
        ? `${JSON.stringify({
            commandLine: 'rg --stats "unsafe|unwrap|expect|panic|TODO|FIXME" src/',
            workingDir: "D:\\primewire-desktop",
            exitCode: 0,
            combinedOutput: Array.from(
              { length: 180 },
              (_, i) => `src/main.rs:${i + 1}    ${"unwrap_or_else(expected) ".repeat(5)}`,
            ).join("\n"),
          }).slice(0, 8_000)}\n…`
        : params.has("shell")
        ? JSON.stringify({
            command_line: "git status",
            workingDir: "D:\\primewire-desktop",
            exitCode: 1,
            combined_output: "fatal: not a git repository\n",
            formatted_output: "fatal: not a git repository\n",
          })
        : '{"ok":true,"items":[1,2],"text":"<script>inert</script>"}',
    ],
    [
      "large",
      "Run large output",
      Array.from(
        { length: 120 },
        (_, i) => `Line ${i}: ${"Long output ".repeat(12)}`,
      ).join("\n"),
    ],
    [
      "monocode",
      "monocode app notes.list",
      '{"ok":true,"result":{"notes":["Ideas"]}}',
    ],
    ["read", "Read src/app.ts", "export const result = 42;"],
    ["search", "Find token", '{"unfinished":…'],
    ["edit", "Edit src/app.ts", "Updated file."],
      ];
  for (const [callId, title, detail] of outputs) {
    const kind = callId === "opencode-find"
      ? "search"
      : callId === "opencode-read"
        ? "read"
        : ["read", "search", "edit"].includes(callId) ? callId : "shell";
    initial = applyHarnessEvent(initial, {
      type: "tool.started",
      callId,
      title,
      kind,
    });
    initial = applyHarnessEvent(initial, {
      type: "tool.updated",
      callId,
      status: "completed",
      detail,
      ...(callId === "large"
        ? { preview: { kind: "shell" as const, output: detail } }
        : {}),
    });
  }
  initial = applyHarnessEvent(initial, {
    type: "tool.started",
    callId: "empty",
    title: "Run empty success",
    kind: "shell",
  });
  initial = applyHarnessEvent(initial, {
    type: "tool.updated",
    callId: "empty",
    status: "in_progress",
    detail: "pending input arguments",
  });
  initial = applyHarnessEvent(initial, {
    type: "tool.updated",
    callId: "empty",
    status: "completed",
  });
  if (params.has("nested"))
    initial.blocks = [
      initial.blocks[0],
      {
        id: "parent",
        role: "tool",
        text: "Review",
        tool: { kind: "agent", status: "completed" },
        agentRun: {
          name: "Review",
          steps: initial.blocks.slice(1).map((block) => ({
            id: block.id,
            kind: "tool",
            text: block.text,
            toolKind: block.tool?.kind,
            status: "completed",
            detail: block.tool?.detail,
            detailIsOutput: block.tool?.detailIsOutput,
          })),
        },
      },
    ];
}

function Fixture() {
  const [session, setSession] = useState(initial);
  Object.assign(window, {
    reviewSteer: () =>
      setSession((s) => {
        // The submit path drains pre-steer events before inserting this boundary.
        s = applyHarnessEvent(s, { type: "message.delta", text: " checklist" });
        return applyHarnessEvent(appendSteerUser(s, "Continue with tests"), {
          type: "message.delta",
          text: "Got it.",
        });
      }),
    reviewAgain: () =>
      setSession((s) =>
        applyHarnessEvent(appendSteerUser(s, "Keep it focused"), {
          type: "message.delta",
          text: "Testing now.",
        }),
      ),
    reviewDelta: () =>
      setSession((s) =>
        applyHarnessEvent(s, {
          type: "message.delta",
          text: " More output while you read earlier history.",
        }),
      ),
    reviewFinish: () => setSession((s) => stopStreaming(s)),
    reviewRestore: () =>
      setSession((s) => ({
        ...s,
        ...JSON.parse(
          JSON.stringify(sanitizeSessionForPersist(stopStreaming(s))),
        ),
      })),
  });
  if (params.has("standalone"))
    return (
      <div className="p-4">
        {session.blocks
          .filter((block) => block.role === "tool")
          .map((block) => (
            <ToolCall key={block.id} block={block} />
          ))}
      </div>
    );
  return (
    <AgentTranscript
      blocks={session.blocks}
      busy={session.busy}
      inlineWork={params.has("mono")}
      bottomAligned={params.has("mono")}
      agentName={params.has("mono") ? "Mono" : undefined}
    />
  );
}
createRoot(document.getElementById("root")!).render(<Fixture />);
