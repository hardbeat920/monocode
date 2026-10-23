import { createServer } from "node:http";
import {
  mkdtempSync,
  mkdirSync,
  writeFileSync,
  readFileSync,
  existsSync,
  rmSync,
} from "node:fs";
import { it } from "vitest";
import { join } from "node:path";
import { tmpdir } from "node:os";
import assert from "node:assert/strict";
import { HostChildBackend } from "./child-backend";
import {
  configureChildBackend,
  acquireHarnessBridge,
} from "../src/integrations/harness/core/child.ts";
import {
  sendOpenCodeTurn,
  respondOpenCodeApproval,
  rewindOpenCodeLastTurn,
  stopOpenCodeSession,
} from "../src/integrations/harness/providers/opencode/opencode.ts";

const BINARY = process.env.MONOCODE_OPENCODE_V1_BINARY;

it.runIf(Boolean(BINARY))(
  "enforces permissions and recovers overflow through an isolated OpenCode v1 server",
  async () => {
    const originalEnvironment = { ...process.env };
    const root = mkdtempSync(join(tmpdir(), "monocode-fixed-opencode-e2e-"));
    for (const name of ["home", "config", "data", "cache", "state"])
      mkdirSync(join(root, name));
    const toolOutputDirectory = join(root, "data", "opencode", "tool-output");
    mkdirSync(toolOutputDirectory, { recursive: true });
    const toolOutputFile = join(toolOutputDirectory, "review-truncated.txt");
    writeFileSync(toolOutputFile, "saved truncated output fixture\n");
    Object.assign(process.env, {
      OPENCODE_TEST_HOME: join(root, "home"),
      XDG_CONFIG_HOME: join(root, "config"),
      XDG_DATA_HOME: join(root, "data"),
      XDG_CACHE_HOME: join(root, "cache"),
      XDG_STATE_HOME: join(root, "state"),
      OPENCODE_AUTH_CONTENT: "{}",
      OPENCODE_DISABLE_SHARE: "true",
      OPENCODE_DISABLE_AUTOUPDATE: "true",
      OPENCODE_SERVER_PASSWORD: "fixture-inherited-password",
      OPENCODE_SERVER_USERNAME: "fixture-inherited-user",
    });
    process.env.OPENCODE_CONFIG_CONTENT = JSON.stringify({
      agent: {
        general: {
          description: "Preserve fixture agent settings",
          permission: { bash: { "*": "allow" } },
        },
      },
      experimental: { primary_tools: ["bash"] },
    });
    let currentCase = "";
    let marker = "";
    let overflowCalls = 0;
    const requests: any[] = [];
    const bridgeRequests: any[] = [];
    const results: any[] = [];
    const mock = createServer(async (req, res) => {
      let raw = "";
      for await (const piece of req) raw += piece;
      const input = JSON.parse(raw);
      requests.push({ case: currentCase, input });
      const messages = input.messages ?? [];
      const text = messages
        .map((message: any) =>
          typeof message.content === "string"
            ? message.content
            : (message.content ?? [])
                .map((part: any) => part.text ?? "")
                .join(" "),
        )
        .join(" ");
      const hasTools = messages.some((m: any) => m.role === "tool");
      let call: any;
      let content = "review complete";
      if (currentCase.startsWith("overflow")) {
        overflowCalls++;
        if (overflowCalls === 1 || currentCase === "overflow-failure") {
          res.writeHead(400, { "Content-Type": "application/json" });
          res.end(
            JSON.stringify({
              error: {
                message: "Input exceeds the context window",
                type: "invalid_request_error",
                code: "context_length_exceeded",
              },
            }),
          );
          return;
        }
        await new Promise((resolve) => setTimeout(resolve, 250));
        content =
          overflowCalls === 2
            ? "Summary retained the original request."
            : "Recovered after compaction.";
      } else if (!hasTools && text.includes("PLAN_TOOL_OUTPUT_READ_CASE")) {
        call = {
          id: "call_review_read",
          type: "function",
          function: {
            name: "read",
            arguments: JSON.stringify({ filePath: toolOutputFile }),
          },
        };
      } else if (!hasTools && text.includes("PLAN_GRANDCHILD_EXPLORE_CASE")) {
        content = "Nested delegation reached the grandchild.";
      } else if (!hasTools && text.includes("PLAN_CHILD_EXPLORE_CASE")) {
        call = {
          id: "call_review_nested_explore",
          type: "function",
          function: {
            name: "task",
            arguments: JSON.stringify({
              description: "review nested Explore",
              prompt: "PLAN_GRANDCHILD_EXPLORE_CASE",
              subagent_type: "explore",
            }),
          },
        };
      } else if (!hasTools && text.includes("PLAN_PARENT_EXPLORE_CASE")) {
        call = {
          id: "call_review_primary_explore",
          type: "function",
          function: {
            name: "task",
            arguments: JSON.stringify({
              description: "review primary Explore",
              prompt: "PLAN_CHILD_EXPLORE_CASE",
              subagent_type: "explore",
            }),
          },
        };
      } else if (!hasTools && text.includes("CHILD_BASH_CASE")) {
        call = {
          id: "call_review_bash",
          type: "function",
          function: {
            name: "bash",
            arguments: JSON.stringify({
              command: "printf child-executed > child-marker.txt",
              description: "Write review marker",
            }),
          },
        };
      } else if (!hasTools && text.includes("PARENT_TASK_CASE")) {
        call = {
          id: "call_review_task",
          type: "function",
          function: {
            name: "task",
            arguments: JSON.stringify({
              description: "review child",
              prompt: "CHILD_BASH_CASE",
              subagent_type: "general",
            }),
          },
        };
      } else if (!hasTools && text.includes("PLAN_PERMISSION_CASE")) {
        call = {
          id: "call_review_edit",
          type: "function",
          function: {
            name: "edit",
            arguments: JSON.stringify({
              filePath: marker,
              oldString: "before",
              newString: "after",
            }),
          },
        };
      }
      const delta = call
        ? { tool_calls: [{ index: 0, ...call }] }
        : { content };
      const chunks = [
        {
          id: "chatcmpl-review",
          object: "chat.completion.chunk",
          created: Math.floor(Date.now() / 1000),
          model: "fake",
          choices: [{ index: 0, delta, finish_reason: null }],
        },
        {
          id: "chatcmpl-review",
          object: "chat.completion.chunk",
          created: Math.floor(Date.now() / 1000),
          model: "fake",
          choices: [
            {
              index: 0,
              delta: {},
              finish_reason: call ? "tool_calls" : "stop",
            },
          ],
          usage: { prompt_tokens: 5, completion_tokens: 5, total_tokens: 10 },
        },
      ];
      res.writeHead(200, { "Content-Type": "text/event-stream" });
      res.end(
        chunks.map((c) => "data: " + JSON.stringify(c) + "\n\n").join("") +
          "data: [DONE]\n\n",
      );
    });
    await new Promise<void>((resolve) => mock.listen(0, "127.0.0.1", resolve));
    const port = (mock.address() as any).port;
    class FixtureBackend extends HostChildBackend {
      async invoke<T>(command: string, args: Record<string, unknown> = {}) {
        if (command === "harness_http") {
          const url = new URL(String(args.url));
          // Name the fixture session to suppress the unrelated automatic title model call.
          if (args.method === "POST" && url.pathname === "/session")
            args = {
              ...args,
              body: JSON.stringify({
                ...JSON.parse(String(args.body)),
                title: "Isolated integration review",
              }),
            };
          bridgeRequests.push({ command, ...args });
        }
        return super.invoke<T>(command, args);
      }
    }
    const backend = new FixtureBackend({ opencode: BINARY! });
    configureChildBackend(backend);
    const release = await acquireHarnessBridge();
    async function runCase(
      name: string,
      intent: "plan" | "build",
      runtimeMode: "full-access" | "supervised",
      text: string,
    ) {
      currentCase = name;
      overflowCalls = 0;
      const cwd = join(root, name);
      mkdirSync(cwd);
      marker = join(cwd, "plan-marker.txt");
      writeFileSync(marker, "before\n");
      writeFileSync(
        join(cwd, "opencode.json"),
        JSON.stringify({
          enabled_providers: ["review-local"],
          provider: {
            "review-local": {
              npm: "@ai-sdk/openai-compatible",
              name: "Review local",
              options: {
                baseURL: `http://127.0.0.1:${port}/v1`,
                apiKey: "review-fixture",
              },
              models: {
                fake: { name: "Fake", limit: { context: 64000, output: 2048 } },
              },
            },
          },
          model: "review-local/fake",
        }),
      );
      const events: any[] = [];
      const approvals: any[] = [];
      const thread = "real-" + name;
      let timeout: ReturnType<typeof setTimeout>;
      const turn = sendOpenCodeTurn({
        sessionId: thread,
        cwd,
        model: "opencode:review-local/fake",
        runtimeMode,
        intent: intent === "plan" ? "plan" : undefined,
        text,
        attachments:
          name === "attachment-warning"
            ? [
                {
                  id: "missing-file",
                  name: "missing.txt",
                  mimeType: "text/plain",
                  kind: "file",
                  size: 0,
                  path: join(cwd, "missing.txt"),
                },
              ]
            : undefined,
        onEvent(event) {
          events.push(event);
          if (event.type === "approval.requested") {
            const decision = event.kind === "agent" ? "allow" : "deny";
            approvals.push({ kind: event.kind, decision, title: event.title });
            respondOpenCodeApproval(thread, event.requestId, decision);
          }
        },
      });
      try {
        await Promise.race([
          turn,
          new Promise((_, reject) => {
            timeout = setTimeout(
              () => reject(new Error(`Timed out ${name}`)),
              45_000,
            );
          }),
        ]);
        const promptRequest = bridgeRequests.findLast((r: any) =>
          String(r.url).includes("/prompt_async"),
        );
        const prompt = JSON.parse(promptRequest.body);
        assert.match(prompt.messageID, /^msg_/);
        const messagesUrl = promptRequest.url.replace(
          "/prompt_async",
          "/message",
        );
        const response = await backend.invoke<any>("harness_http", {
          method: "GET",
          url: messagesUrl,
        });
        const messages = JSON.parse(response.body);
        assert(
          messages.some(
            (m: any) =>
              m.info.id === prompt.messageID && m.info.role === "user",
          ),
          "Explicit provider message ID persisted",
        );
        assert(
          events.some((e) => e.type === "message.completed"),
          "Provider reports completed turn",
        );
        if (name !== "overflow-failure")
          assert(
            !events.some((e) => e.type === "session.error"),
            "Turn should have no fatal provider event",
          );
        const agentUrl = new URL(promptRequest.url);
        agentUrl.pathname = "/agent";
        const agentResponse = await backend.invoke<any>("harness_http", {
          method: "GET",
          url: agentUrl.toString(),
        });
        assert.equal(
          JSON.parse(agentResponse.body).find(
            (agent: any) => agent.name === "general",
          )?.description,
          "Preserve fixture agent settings",
        );
        if (intent === "plan" || runtimeMode !== "full-access") {
          const general = JSON.parse(agentResponse.body).find(
            (agent: any) => agent.name === "general",
          );
          const trustedGlob = join(toolOutputDirectory, "*");
          assert.equal(
            general.permission.findLast(
              (rule: any) =>
                rule.permission === "external_directory" &&
                rule.pattern === trustedGlob,
            )?.action,
            "allow",
            "The owned tool-output directory remains readable",
          );
          assert.equal(
            general.permission.findLast(
              (rule: any) =>
                rule.permission === "external_directory" &&
                rule.pattern === "*",
            )?.action,
            intent === "plan" ? "deny" : "ask",
          );
          const sessionRequest = bridgeRequests.findLast(
            (request: any) =>
              request.method === "POST" &&
              new URL(request.url).pathname === "/session",
          );
          assert(
            JSON.parse(sessionRequest.body).permission.some(
              (rule: any) =>
                rule.permission === "external_directory" &&
                rule.pattern === trustedGlob &&
                rule.action === "allow",
            ),
            "Session permissions preserve the same exact tool-output exception",
          );
        }
        if (name === "plan-output") {
          assert.equal(approvals.length, 0);
          const toolParts = messages.flatMap(
            (message: any) =>
              message.parts?.filter((part: any) => part.type === "tool") ?? [],
          );
          assert(
            toolParts.some(
              (part: any) =>
                part.tool === "read" &&
                part.state?.status === "completed" &&
                part.state.output?.includes("saved truncated output fixture"),
            ),
            `Plan can read the owned truncated tool output. ${JSON.stringify(toolParts)}`,
          );
        }
        if (name === "plan-task") {
          assert.equal(approvals.length, 0);
          const childResponse = await backend.invoke<any>("harness_http", {
            method: "GET",
            url: messagesUrl.replace("/message", "/children"),
          });
          const children = JSON.parse(childResponse.body);
          assert.equal(
            children.length,
            1,
            "Primary Plan task creates an Explore child",
          );
          const nestedResponse = await backend.invoke<any>("harness_http", {
            method: "GET",
            url: messagesUrl.replace(
              /\/session\/[^/]+\/message/,
              `/session/${children[0].id}/children`,
            ),
          });
          assert.deepEqual(
            JSON.parse(nestedResponse.body),
            [],
            "The Explore child cannot create another Explore session",
          );
          const childRequests = requests.filter(
            (request) =>
              request.case === name &&
              request.input.messages?.some((message: any) => {
                if (message.role !== "user") return false;
                const content =
                  typeof message.content === "string"
                    ? message.content
                    : (message.content ?? [])
                        .map((part: any) => part.text ?? "")
                        .join(" ");
                return content.includes("PLAN_CHILD_EXPLORE_CASE");
              }),
          );
          assert(
            childRequests.length > 0,
            "The real Explore child reached the local provider",
          );
          assert(
            childRequests.every(
              (request) =>
                !request.input.tools?.some(
                  (tool: any) => tool.function.name === "task",
                ),
            ),
            "Explore children never receive the Task tool",
          );
          assert(
            messages.some((message: any) =>
              message.parts?.some(
                (part: any) =>
                  part.type === "tool" &&
                  part.tool === "task" &&
                  part.state?.status === "completed",
              ),
            ),
            "The legitimate primary Explore task completes",
          );
        }
        if (name === "plan") {
          assert.equal(readFileSync(marker, "utf8"), "before\n");
          assert(
            !requests
              .filter((r) => r.case === name)
              .some((r) =>
                r.input.tools?.some((t: any) => t.function.name === "edit"),
              ),
            "Plan removes edit tool",
          );
          const originalAssistant = messages.find(
            (message: any) =>
              message.info.role === "assistant" &&
              message.info.parentID === prompt.messageID,
          )?.info.id;
          assert(originalAssistant);
          const input = {
            sessionId: thread,
            cwd,
            model: "opencode:review-local/fake",
            runtimeMode,
            intent: "plan" as const,
            onEvent: (event: unknown) => events.push(event),
          };
          await sendOpenCodeTurn({ ...input, text: "PLAN_SECOND_TURN_CASE" });
          const secondRequest = bridgeRequests.findLast((request: any) =>
            String(request.url).includes("/prompt_async"),
          );
          const secondMessageID = JSON.parse(secondRequest.body).messageID;
          await rewindOpenCodeLastTurn(input);
          await sendOpenCodeTurn({ ...input, text: "PLAN_REPLACEMENT_CASE" });
          const afterRewind = JSON.parse(
            (
              await backend.invoke<any>("harness_http", {
                method: "GET",
                url: messagesUrl,
              })
            ).body,
          );
          assert(
            afterRewind.some(
              (message: any) => message.info.id === originalAssistant,
            ),
            "Rewind preserves the earlier assistant turn",
          );
          assert(
            !afterRewind.some(
              (message: any) => message.info.id === secondMessageID,
            ),
            "Rewind removes the replaced provider user turn",
          );
          assert.equal(readFileSync(marker, "utf8"), "before\n");
        }
        if (name === "supervised") {
          assert(
            approvals.some((a) => a.kind === "agent" && a.decision === "allow"),
            "Parent task approval",
          );
          assert(
            approvals.some((a) => a.kind === "shell" && a.decision === "deny"),
            "Child bash approval denied",
          );
          assert(
            !existsSync(join(cwd, "child-marker.txt")),
            "Denied child must not write marker",
          );
        }
        if (name === "overflow") {
          assert.equal(overflowCalls, 3);
          assert(
            events.some(
              (e) =>
                e.type === "status" && e.text.includes("compacting context"),
            ),
          );
          assert(
            events.some(
              (e) =>
                e.type === "message.part" &&
                e.text === "Recovered after compaction.",
            ),
          );
          assert(
            !events.some(
              (e) =>
                e.type === "message.part" &&
                e.text.includes("Summary retained"),
            ),
            "Compaction summary stays hidden",
          );
        }
        if (name === "overflow-failure") {
          assert.equal(overflowCalls, 2);
          assert(
            events.some(
              (e) =>
                e.type === "session.error" &&
                e.message.includes("too large to compact"),
            ),
            "Terminal failed compaction must report its durable error",
          );
        }
        if (name === "attachment-warning") {
          assert(
            events.some(
              (event) =>
                event.type === "status" && event.text.includes("missing.txt"),
            ),
            "Attachment read failure is reported as a warning",
          );
          assert(
            messages.some((message: any) =>
              message.parts?.some(
                (part: any) =>
                  part.synthetic &&
                  part.text?.startsWith("Read tool failed to read"),
              ),
            ),
            "OpenCode retained the recoverable file-read failure in the prompt",
          );
          assert(
            events.some(
              (event) =>
                event.type === "message.part" &&
                event.text === "review complete",
            ),
            "The provider answer follows the file warning",
          );
        }
        results.push({
          name,
          approvals,
          messageID: prompt.messageID,
          marker: readFileSync(marker, "utf8"),
          messages,
          events,
        });
        console.log(
          JSON.stringify({
            case: name,
            passed: true,
            approvals,
            messageID: prompt.messageID,
            events: events.length,
            requests: requests.filter((r) => r.case === name).length,
          }),
        );
      } finally {
        clearTimeout(timeout!);
        await stopOpenCodeSession(thread);
        await turn.catch(() => undefined);
      }
    }
    try {
      console.log("fixture " + root);
      await runCase("plan", "plan", "full-access", "PLAN_PERMISSION_CASE");
      await runCase(
        "plan-output",
        "plan",
        "full-access",
        "PLAN_TOOL_OUTPUT_READ_CASE",
      );
      await runCase(
        "plan-task",
        "plan",
        "full-access",
        "PLAN_PARENT_EXPLORE_CASE",
      );
      await runCase("supervised", "build", "supervised", "PARENT_TASK_CASE");
      await runCase(
        "overflow",
        "build",
        "full-access",
        "OVERFLOW_RECOVERY_CASE",
      );
      await runCase(
        "overflow-failure",
        "build",
        "full-access",
        "OVERFLOW_FAILURE_CASE",
      );
      await runCase(
        "attachment-warning",
        "build",
        "full-access",
        "ATTACHMENT_WARNING_CASE",
      );
    } finally {
      writeFileSync(
        join(root, "requests.json"),
        JSON.stringify(requests, null, 2),
      );
      writeFileSync(
        join(root, "bridge-requests.json"),
        JSON.stringify(bridgeRequests, null, 2),
      );
      writeFileSync(
        join(root, "results.json"),
        JSON.stringify(results, null, 2),
      );
      await backend.close();
      release();
      await new Promise<void>((resolve) => mock.close(() => resolve()));
      for (const key of Object.keys(process.env))
        if (!(key in originalEnvironment)) delete process.env[key];
      Object.assign(process.env, originalEnvironment);
      rmSync(root, { recursive: true, force: true });
    }
  },
  180_000,
);
