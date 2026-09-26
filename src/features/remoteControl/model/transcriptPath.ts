/**
 * Where the CLI writes a conversation: `~/.claude/projects/<encoded>/<id>.jsonl`.
 *
 * The encoding mirrors `claude_project_dir` in `src-tauri/src/fs.rs`, which maps
 * `|c| if c.is_ascii_alphanumeric() { c } else { '-' }`. That function is private
 * and not exposed as a command, so it cannot be called from here; the cases in
 * this module's test are copied from its own Rust tests so the two implementations
 * cannot drift apart without a test going red.
 */

/** Every character that is not an ASCII letter or digit becomes `-`. */
export function encodeProjectDir(cwd: string): string {
  // Rust maps over `chars()`, i.e. code points. Iterating the string with
  // spread does the same, so an astral character collapses to one `-` instead
  // of one per UTF-16 half.
  return [...cwd]
    .map((ch) => (/^[0-9A-Za-z]$/.test(ch) ? ch : "-"))
    .join("");
}

function joinHome(home: string, rest: string): string {
  return `${home.replace(/[/\\]+$/, "")}/${rest}`;
}

export function claudeProjectDir(home: string, cwd: string): string {
  return joinHome(home, `.claude/projects/${encodeProjectDir(cwd)}`);
}

export function claudeTranscriptPath(
  home: string,
  cwd: string,
  providerSessionId: string,
): string {
  return `${claudeProjectDir(home, cwd)}/${providerSessionId}.jsonl`;
}
