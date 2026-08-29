import {
  acquireHarnessBridge,
  killChild,
  spawnChild,
  unwatchChild,
  watchChild,
  writeChild,
} from "./child";
import { PiRpc } from "./piClient";
import { PI_FLAVOR } from "./piFlavor";
import {
  asRecord,
  buildPiSpawnArgs,
  extensionUiResponse,
  needsExtensionUiReply,
  parseExtensionUiRequest,
} from "./piProtocol";

const REQUEST_TIMEOUT_MS = 45_000;

export type PiSkillCommand = {
  name: string;
  description: string;
  invocation: string;
  source: "pi";
};

export async function discoverPiSkills(
  cwd: string,
): Promise<PiSkillCommand[]> {
  const { path } = await PI_FLAVOR.resolveBinary();
  const releaseBridge = await acquireHarnessBridge();
  const childId = `monocode-pi-skills-${crypto.randomUUID()}`;
  const replyToUi = (record: Record<string, unknown>) => {
    const request = parseExtensionUiRequest(record);
    if (!request || !needsExtensionUiReply(request)) return;
    void writeChild(
      childId,
      JSON.stringify(extensionUiResponse(request, "deny")),
    ).catch(() => undefined);
  };
  const rpc = new PiRpc(childId, replyToUi, PI_FLAVOR.label);

  try {
    watchChild(
      childId,
      (line) => rpc.pushLine(line),
      () => rpc.close(new Error(`${PI_FLAVOR.label} skill probe exited`)),
    );
    await spawnChild(
      childId,
      path,
      buildPiSpawnArgs(PI_FLAVOR, { noSession: true }),
      cwd,
    );
    const response = await rpc.request(
      { type: "get_commands" },
      REQUEST_TIMEOUT_MS,
    );
    return piSkillsFromRpcData(asRecord(response)?.data);
  } finally {
    rpc.close();
    unwatchChild(childId);
    await killChild(childId).catch(() => undefined);
    releaseBridge();
  }
}

export function piSkillsFromRpcData(data: unknown): PiSkillCommand[] {
  const commands = asRecord(data)?.commands;
  if (!Array.isArray(commands)) {
    throw new Error("Pi get_commands returned no commands array");
  }

  const seen = new Set<string>();
  const skills: PiSkillCommand[] = [];
  for (const value of commands) {
    const command = asRecord(value);
    const invocation = command?.name;
    if (
      command?.source !== "skill" ||
      typeof invocation !== "string" ||
      !invocation.startsWith("skill:") ||
      seen.has(invocation)
    ) {
      continue;
    }
    const name = invocation.slice("skill:".length);
    if (!name) continue;
    seen.add(invocation);
    skills.push({
      name,
      description:
        typeof command.description === "string" ? command.description : "",
      invocation,
      source: "pi",
    });
  }
  return skills;
}
