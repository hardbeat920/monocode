import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { randomUUID } from "node:crypto";

const definitions = /* TOOL_DEFINITIONS */ [];
// What Claude and Codex get in their system prompt: everything the MonoCode
// server offers, or only the thread part when Pi gets just the thread tools.
const guidance = /* GUIDANCE */ "";
const threadGuidance = /* THREAD_GUIDANCE */ "";
const execute = promisify(execFile);

export default async function (pi) {
  if (process.env.MONOCODE_THREADS !== "1") return;
  const executable = process.env.MONOCODE_VISUAL_EXECUTABLE;
  if (!executable) throw new Error("MonoCode executable is unavailable");
  const appendToSystemPrompt = (text) => {
    if (typeof pi.on === "function") pi.on("before_agent_start", (event) => ({ systemPrompt: `${event.systemPrompt}\n\n${text}` }));
  };
  if (typeof pi.registerMcpServer === "function") {
    pi.registerMcpServer("monocode", { command: executable, args: ["visual-mcp"], exposure: "direct", timeout: 90 });
    appendToSystemPrompt(guidance);
    return;
  }
  appendToSystemPrompt(threadGuidance);
  const { Type } = await import("typebox").catch(() => import("@sinclair/typebox"));
  const schema = (value) => {
    if (value.enum) return Type.Union(value.enum.map((item) => Type.Literal(item)));
    if (Array.isArray(value.type)) return Type.Union(value.type.map((type) => schema({ ...value, type })));
    if (value.type === "object") {
      if (value.additionalProperties?.type) return Type.Record(Type.String(), schema(value.additionalProperties));
      return Type.Object(Object.fromEntries(Object.entries(value.properties ?? {}).map(([key, child]) =>
        [key, (value.required ?? []).includes(key) ? schema(child) : Type.Optional(schema(child))])), { additionalProperties: false });
    }
    if (value.type === "integer") return Type.Integer(value);
    return Type.String(value);
  };
  for (const tool of definitions) {
    pi.registerTool({
      name: tool.name,
      label: tool.name,
      description: tool.description,
      parameters: schema(tool.inputSchema),
      async execute(_id, input, signal) {
        const { requestId = randomUUID(), ...args } = input;
        const action = tool.name === "new_thread" ? "threads.new" : tool.name.replace("thread_", "threads.");
        const { stdout } = await execute(executable, ["app", action, "--json", JSON.stringify(args), "--request-id", requestId], { signal, timeout: 65000, maxBuffer: 4194304 });
        const result = JSON.parse(stdout);
        if (!result.ok) throw new Error(result.error ?? "Thread request failed");
        return { content: [{ type: "text", text: JSON.stringify(result.result) }], details: result.result };
      },
    });
  }
  pi.on("session_start", () => pi.setActiveTools([...new Set([...pi.getActiveTools(), ...definitions.map((tool) => tool.name)])]));
}
