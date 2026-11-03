// Keep these aliases aligned with AgentMarkdown's LANGUAGE_FILE_NAMES on desktop.
const LANGUAGE_EXTENSIONS: Record<string, string> = {
  bash: "sh", "c++": "cpp", csharp: "cs", javascript: "js",
  markdown: "md", python: "py", ruby: "rb", rust: "rs",
  shell: "sh", typescript: "ts", yml: "yaml", zsh: "sh",
};

export function codeFenceHeader(language: string) {
  const label = language || "text";
  const key = label.toLowerCase();
  return { label, fileName: `code.${LANGUAGE_EXTENSIONS[key] ?? key}` };
}
