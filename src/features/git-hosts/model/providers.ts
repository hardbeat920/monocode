import { parseRemoteUrl, validRepoSlug } from "./remoteUrl";
import type { GitHostId } from "./types";

/** How a provider appears in the UI. Adding a service means adding an entry
 * here plus its desktop and machine host providers. */
export type GitHostUi = {
  id: GitHostId;
  label: string;
  /** The host name repository URLs use. */
  domain: string;
  placeholder: string;
};

export const GIT_HOST_UI: readonly GitHostUi[] = [
  {
    id: "github",
    label: "GitHub",
    domain: "github.com",
    placeholder: "owner/name or a github.com URL",
  },
];

export const gitHostUi = (id: GitHostId): GitHostUi =>
  GIT_HOST_UI.find((entry) => entry.id === id)!;

/** `owner/name` from what was typed: a slug, or a repository URL on the
 * provider's domain (including links to a branch, file or pull request). */
export function parseRepoInput(provider: GitHostUi, text: string): string | null {
  const value = text.trim().replace(/[?#].*$/, "");
  const bareDomain = value.toLowerCase().startsWith(`${provider.domain}/`);
  if (!bareDomain && validRepoSlug(value)) return value;
  const remote = parseRemoteUrl(bareDomain ? `https://${value}` : value);
  if (!remote || remote.host !== provider.domain) return null;
  const [owner, name] = remote.path.split("/");
  const slug = `${owner}/${name}`;
  return name && validRepoSlug(slug) ? slug : null;
}
