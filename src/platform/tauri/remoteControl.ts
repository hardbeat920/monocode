import { invoke } from "@tauri-apps/api/core";

/**
 * Kill any earlier `claude --resume <id> --remote-control …` for this
 * conversation, and say how many there were. The CLI refuses a second
 * interactive process on a session that already has one and exits with code 1.
 */
export function reapRemoteControlSession(
  providerSessionId: string,
): Promise<number> {
  return invoke<number>("reap_remote_control_session", { providerSessionId });
}
