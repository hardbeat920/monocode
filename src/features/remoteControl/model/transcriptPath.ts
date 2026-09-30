/**
 * Where the CLI writes a conversation: `~/.claude/projects/<encoded>/<id>.jsonl`.
 *
 * ## This is a second copy of a rule that also lives in Rust
 *
 * `claude_project_dir` in `src-tauri/src/fs.rs` encodes the same thing, mapping
 * `|c| if c.is_ascii_alphanumeric() { c } else { '-' }`. It is a private `fn`
 * rather than a `#[tauri::command]`, so it cannot be called from here — hence
 * the duplicate. **Change one and you must change the other**, or MonoCode and
 * the directory it looks in will disagree, and the failure is silent: the mirror
 * finds no file and simply shows nothing.
 *
 * The cases in this module's test are copied from that function's own Rust tests
 * so a divergence shows up as a red test rather than an empty session.
 */

/** Every character that is not an ASCII letter or digit becomes `-`. */
export function encodeProjectDir(cwd: string): string {
  // Rust maps over `chars()`, i.e. code points, so this must too. A regex over
  // the string (`cwd.replace(/[^0-9A-Za-z]/g, "-")`) would work on UTF-16 units
  // and spend two dashes on an astral character where Rust spends one, giving a
  // directory name that does not exist. Iterating with spread yields code points.
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
