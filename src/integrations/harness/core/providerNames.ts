// Display names for upstream providers that multi-provider harnesses
// (OpenCode, Pi/omp, Hermes) report only as slugs.
const PROVIDER_NAMES: Record<string, string> = {
  anthropic: "Anthropic",
  deepseek: "DeepSeek",
  "github-copilot": "GitHub Copilot",
  "google-antigravity": "Antigravity",
  "google-gemini-cli": "Gemini CLI",
  "lm-studio": "LM Studio",
  ollama: "Ollama",
  openai: "OpenAI",
  "openai-codex": "OpenAI Codex",
  opencode: "OpenCode",
  "opencode-go": "OpenCode Go",
  openrouter: "OpenRouter",
  xai: "xAI",
};

/** Resolve a provider slug to its display name, title-casing unknown providers. */
export function upstreamProviderName(providerID: string): string {
  return PROVIDER_NAMES[providerID] ?? titleCaseSlug(providerID);
}

/** Capitalize slug segments separated by hyphens, underscores, or slashes. */
export function titleCaseSlug(value: string): string {
  const segments: string[] = [];
  for (const segment of value.split(/[-_/]+/)) {
    if (segment.length > 0) {
      segments.push(segment.charAt(0).toUpperCase() + segment.slice(1));
    }
  }
  return segments.join(" ");
}
