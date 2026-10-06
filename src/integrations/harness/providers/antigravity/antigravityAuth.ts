import { AcpClient } from "../../core/acp";
import {
  killChild,
  resolveAntigravityBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import { antigravitySpawnCwd } from "./antigravityProtocol";

/** Select and authenticate the official ACP server's Google account method.
 * Running the interactive agy CLI alone does not select ACP's auth.type. */
export async function loginAntigravity(childId: string): Promise<void> {
  const { path, args } = await resolveAntigravityBinary();
  const acp = new AcpClient(childId, {
    onRequest: (id, method) => {
      void acp
        .respondError(id, {
          code: -32601,
          message: `Method not found: ${method}`,
        })
        .catch(() => undefined);
    },
  });
  await killChild(childId).catch(() => undefined);
  watchChild(
    childId,
    (line) => acp.pushLine(line),
    () => acp.close(new Error("Antigravity sign-in exited before completing.")),
  );
  try {
    await spawnChild(
      childId,
      path,
      args,
      antigravitySpawnCwd(path, "."),
      undefined,
      "antigravity",
    );
    const result = await acp.request<{ authMethods?: { id: string }[] }>(
      "initialize",
      {
        protocolVersion: 1,
        clientCapabilities: {},
        clientInfo: { name: "monocode", version: "0.1.0" },
      },
      12_000,
    );
    if (!result.authMethods?.some((method) => method.id === "oauth-personal")) {
      throw new Error(
        "This Antigravity ACP server does not offer Google account sign-in.",
      );
    }
    // The official server opens the browser when necessary and persists its
    // selected method and credentials. Never copy tokens from the CLI.
    await acp.request(
      "authenticate",
      { methodId: "oauth-personal" },
      10 * 60_000,
    );
  } finally {
    acp.close();
    unwatchChild(childId);
    await killChild(childId).catch(() => undefined);
  }
}
