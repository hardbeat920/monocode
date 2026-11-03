import { homeDir } from "../../../../platform/tauri/fs";
import { execChild, readHarnessTextFile } from "../../core/child";
import {
  devinApiKeyFromCredentials,
  devinCredentialsCandidates,
  devinCredentialsPathFromStatus,
} from "./devinProtocol";

let credentialsPath: string | null = null;

/**
 * Devin's ACP server ignores the CLI's stored login and waits for its host to
 * authenticate. Reuse the key `devin auth login` saved — on the machine that
 * runs the child, so remote hosts read their own login — instead of asking
 * for a second sign-in. Returns null when the CLI has never been logged in.
 */
export async function readDevinApiKey(binaryPath: string): Promise<string | null> {
  for (const path of await credentialPaths(binaryPath)) {
    try {
      const key = devinApiKeyFromCredentials(await readHarnessTextFile(path));
      if (key) {
        credentialsPath = path;
        return key;
      }
    } catch {
      // Missing or unreadable; try the next location.
    }
  }
  return null;
}

async function credentialPaths(binaryPath: string): Promise<string[]> {
  const paths: string[] = [];
  if (credentialsPath) paths.push(credentialsPath);
  try {
    const status = await execChild(binaryPath, ["auth", "status"], undefined, "devin");
    const reported = devinCredentialsPathFromStatus(status);
    if (reported) paths.push(reported);
  } catch {
    // Older CLIs or a failing status call still leave the default locations.
  }
  try {
    paths.push(...devinCredentialsCandidates(await homeDir()));
  } catch {
    // Headless hosts have no desktop home lookup; status covers them.
  }
  return [...new Set(paths)];
}

/** Test seam. */
export function resetDevinAuthCache(): void {
  credentialsPath = null;
}
