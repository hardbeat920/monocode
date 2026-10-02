//! Port of src/integrations/harness/core/shellIntent.ts.
//!
//! Visual only: turn a bash argv into Read, Find, or List when the intent is
//! obvious.

use crate::js;
use crate::reducer::js_regex::{is_word_byte, js_regex, nonempty, starts_with_word_then_text};

/// The action a shell command reads as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShellVerb {
    Read,
    Find,
    List,
    Edit,
    Write,
}

impl ShellVerb {
    pub fn as_str(self) -> &'static str {
        match self {
            ShellVerb::Read => "Read",
            ShellVerb::Find => "Find",
            ShellVerb::List => "List",
            ShellVerb::Edit => "Edit",
            ShellVerb::Write => "Write",
        }
    }
}

/// `ShellIntent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellIntent {
    pub verb: ShellVerb,
    pub path: Option<String>,
    pub query: Option<String>,
    pub start_line: Option<i64>,
}

impl ShellIntent {
    fn new(verb: ShellVerb, path: Option<String>) -> Self {
        Self {
            verb,
            path,
            query: None,
            start_line: None,
        }
    }
}

/// Long scripts stay as the command. This runs on every transcript row.
const MAX_COMMAND_CHARS: usize = 2000;

/// `READABLE_BIN`: the command names a binary this classifier understands.
fn has_readable_bin(text: &str) -> bool {
    const BINS: [&str; 21] = [
        "cat", "bat", "batcat", "nl", "less", "more", "tac", "head", "tail", "sed", "grep",
        "egrep", "fgrep", "rgrep", "rg", "ag", "ack", "find", "ls", "tree", "tee",
    ];
    // `\bname\b` holds exactly when `name` is a whole run of word characters.
    text.split(|c: char| !(c.is_ascii() && is_word_byte(c as u8)))
        .any(|word| BINS.contains(&word))
}

/// `inferShellIntent`: best-effort label for a shell command. `None` when the
/// command should stay as typed (git, npm, scripts, mixed opaque work).
pub fn infer_shell_intent(command: &str) -> Option<ShellIntent> {
    let text = unwrap_shell_command(command);
    if text.is_empty() || js::len(&text) > MAX_COMMAND_CHARS {
        return None;
    }
    if starts_with_word_then_text(&text, &["Read", "Find", "List", "Edit", "Write"], false) {
        return None;
    }
    if looks_unsafe(&text) {
        return None;
    }
    let write = extract_write_redirect(&text);
    if !has_readable_bin(&text) && write.is_none() {
        return None;
    }

    let mut intents: Vec<ShellIntent> = Vec::new();
    for chain in split_top_level(&text, chain_sep) {
        let stages = split_top_level(chain, pipe_sep);
        let mut pipe_intent = None;
        for stage in stages {
            let tokens = tokenize(stage)?;
            if tokens.is_empty() {
                return None;
            }
            let argv: Vec<String> = tokens.into_iter().map(|token| token.value).collect();
            match classify_argv(&argv) {
                Classified::Opaque => return None,
                Classified::Noise => continue,
                Classified::Intent(intent) => pipe_intent = Some(intent),
            }
        }
        if let Some(intent) = pipe_intent {
            intents.push(intent);
        }
    }

    if let Some(write) = write {
        let verb = if write.append {
            ShellVerb::Edit
        } else {
            ShellVerb::Write
        };
        return Some(ShellIntent::new(verb, Some(write.path)));
    }
    let ranked = || intents.iter().rev();
    ranked()
        .find(|item| matches!(item.verb, ShellVerb::Edit | ShellVerb::Write))
        .or_else(|| ranked().find(|item| matches!(item.verb, ShellVerb::Read | ShellVerb::Find)))
        .or_else(|| ranked().next())
        .cloned()
}

/// The launchers [`unwrap_shell_command`] looks inside (`SHELL_WRAPPERS`).
#[derive(Clone, Copy)]
enum ShellWrapper {
    Posix,
    PowerShell,
    Cmd,
}

impl ShellWrapper {
    fn for_executable(name: &str) -> Option<Self> {
        match name {
            "sh" | "bash" | "zsh" | "dash" | "ksh" => Some(Self::Posix),
            "powershell" | "powershell.exe" | "pwsh" | "pwsh.exe" => Some(Self::PowerShell),
            "cmd" | "cmd.exe" => Some(Self::Cmd),
            _ => None,
        }
    }

    fn is_command_flag(self, value: &str) -> bool {
        match self {
            // /^(?:--command|-[a-z]*c[a-z]*)$/i
            Self::Posix => {
                value.eq_ignore_ascii_case("--command")
                    || value.strip_prefix('-').is_some_and(|rest| {
                        rest.bytes().all(|b| b.is_ascii_alphabetic())
                            && rest.bytes().any(|b| b.eq_ignore_ascii_case(&b'c'))
                    })
            }
            // /^-(?:command|c)$/i
            Self::PowerShell => {
                value.eq_ignore_ascii_case("-command") || value.eq_ignore_ascii_case("-c")
            }
            // /^\/c$/i
            Self::Cmd => value.eq_ignore_ascii_case("/c"),
        }
    }

    /// `optionBoundary`: only PowerShell has one, `/^-(?:file|f)$/i`.
    fn is_option_boundary(self, value: &str) -> bool {
        matches!(self, Self::PowerShell)
            && (value.eq_ignore_ascii_case("-file") || value.eq_ignore_ascii_case("-f"))
    }

    fn consume_remainder(self) -> bool {
        !matches!(self, Self::Posix)
    }
}

/// `unwrapShellCommand`.
///
/// Codex may expose a command through the argv used to launch the user's
/// shell, for example `/bin/zsh -lc "cat package.json"` or
/// `pwsh.exe -Command "Get-Content package.json"`. The launcher is transport
/// noise for this visual-only classifier, so inspect the script it was given.
pub fn unwrap_shell_command(command: &str) -> String {
    let mut current = js::trim(command).to_string();
    for _ in 0..2 {
        let Some(tokens) = tokenize(&current) else {
            break;
        };
        if tokens.len() < 3 {
            break;
        }
        let executable_token = &tokens[0];
        let raw_executable = &current[executable_token.start..executable_token.end];
        let executable_quote = current[executable_token.start..].chars().next();
        let executable = if matches!(executable_quote, Some('"' | '\''))
            && raw_executable.chars().next() == raw_executable.chars().next_back()
        {
            raw_executable
                .get(1..raw_executable.len().saturating_sub(1))
                .unwrap_or("")
        } else {
            raw_executable
        };
        let Some(wrapper) = ShellWrapper::for_executable(&bin_name(executable)) else {
            break;
        };
        let mut flag_index = None;
        for (index, token) in tokens.iter().enumerate().skip(1) {
            if wrapper.is_option_boundary(&token.value) {
                break;
            }
            if wrapper.is_command_flag(&token.value) {
                flag_index = Some(index);
                break;
            }
        }
        let Some(flag_index) = flag_index else {
            break;
        };
        let Some(command_token) = tokens.get(flag_index + 1) else {
            break;
        };
        let remainder = js::trim(&current[command_token.start..]);
        let command_quote = current[command_token.start..].chars().next();
        let command_is_sole_quoted_token = matches!(command_quote, Some('"' | '\''))
            && current[..command_token.end].chars().next_back() == command_quote
            && tokens.len() == flag_index + 2;
        let script = if wrapper.consume_remainder() && !command_is_sole_quoted_token {
            remainder
        } else {
            js::trim(&command_token.value)
        };
        if script.is_empty() || script == current {
            break;
        }
        current = script.to_string();
    }
    current
}

/// `formatShellIntent`.
pub fn format_shell_intent(
    intent: &ShellIntent,
    path: Option<&str>,
    query: Option<&str>,
) -> Option<String> {
    if intent.verb == ShellVerb::Find {
        let q = nonempty(query).or(nonempty(intent.query.as_deref()))?;
        return Some(format!("Find {q}"));
    }
    let target = nonempty(path).or(nonempty(intent.path.as_deref()))?;
    Some(format!("{} {target}", intent.verb.as_str()))
}

/// `rewriteReadableTitle`: re-apply a stored Read, Find, List, or Edit title
/// when the row already went through ingest.
pub fn rewrite_readable_title(
    title: &str,
    path: Option<&str>,
    query: Option<&str>,
) -> Option<String> {
    let found = js_regex!(r"^(?i-u:(read|find|list|edit|write)){S}+({DOT}+)$").captures(title)?;
    let word = &found[1];
    let verb = format!("{}{}", word[..1].to_uppercase(), word[1..].to_lowercase());
    let rest = &found[2];
    if verb == "Find" {
        return Some(format!("Find {}", nonempty(query).unwrap_or(rest)));
    }
    Some(format!("{verb} {}", nonempty(path).unwrap_or(rest)))
}

/// `Classified`: an intent, a stage that does not change the label, or a
/// stage that makes the whole command opaque.
enum Classified {
    Intent(ShellIntent),
    Noise,
    Opaque,
}

fn classify_argv(argv: &[String]) -> Classified {
    let bin = bin_name(&argv[0]);
    match bin.as_str() {
        "cd" | "echo" | "printf" | "pwd" | "true" | "false" | "clear" | ":" | "wc" | "sleep"
        | "export" | "unset" | "alias" | "wait" => Classified::Noise,
        "cat" | "bat" | "batcat" | "nl" | "less" | "more" | "tac" => read_intent(argv),
        "head" | "tail" => head_tail_intent(argv),
        "sed" => sed_intent(argv),
        "tee" => tee_intent(argv),
        "rg" if argv.iter().any(|arg| arg == "--files") => Classified::Intent(ShellIntent {
            verb: ShellVerb::Find,
            path: None,
            query: Some("files".into()),
            start_line: None,
        }),
        "grep" | "egrep" | "fgrep" | "rgrep" | "rg" | "ag" | "ack" => grep_intent(argv),
        "find" => find_intent(argv),
        "ls" | "tree" => list_intent(argv),
        _ => Classified::Opaque,
    }
}

/// `GREP_VALUE_FLAGS`.
fn is_grep_value_flag(arg: &str) -> bool {
    matches!(
        arg,
        "-e" | "--regexp"
            | "-f"
            | "--file"
            | "-A"
            | "--after-context"
            | "-B"
            | "--before-context"
            | "-C"
            | "--context"
            | "-m"
            | "--max-count"
            | "-d"
            | "--directories"
            | "-D"
            | "--devices"
            | "--include"
            | "--exclude"
            | "--exclude-dir"
            | "-g"
            | "--glob"
            | "-t"
            | "--type"
            | "-j"
            | "--threads"
            | "--max-filesize"
            | "--max-depth"
            | "--max-columns"
    )
}

/// `FIND_VALUE_FLAGS`.
fn is_find_value_flag(arg: &str) -> bool {
    matches!(
        arg,
        "-name"
            | "-iname"
            | "-path"
            | "-ipath"
            | "-wholename"
            | "-iwholename"
            | "-regex"
            | "-iregex"
            | "-type"
            | "-mtime"
            | "-mmin"
            | "-ctime"
            | "-cmin"
            | "-atime"
            | "-amin"
            | "-size"
            | "-maxdepth"
            | "-mindepth"
            | "-user"
            | "-group"
            | "-perm"
            | "-exec"
            | "-execdir"
            | "-ok"
            | "-printf"
    )
}

fn is_flag(arg: &str) -> bool {
    arg.starts_with('-') && arg != "-"
}

fn read_intent(argv: &[String]) -> Classified {
    let files = positional_args(argv, &[]);
    match last_file(&files) {
        Some(path) => Classified::Intent(ShellIntent::new(ShellVerb::Read, Some(path))),
        None => Classified::Noise,
    }
}

fn head_tail_intent(argv: &[String]) -> Classified {
    let mut files: Vec<&str> = Vec::new();
    let mut i = 1;
    while i < argv.len() {
        let arg = argv[i].as_str();
        if arg == "--" {
            files.extend(argv[i + 1..].iter().map(String::as_str));
            break;
        }
        let dash_count = arg
            .strip_prefix('-')
            .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()));
        if dash_count || matches!(arg, "-n" | "-c" | "-q" | "-v") {
            if matches!(arg, "-n" | "-c") {
                i += 1;
            }
            i += 1;
            continue;
        }
        if arg.starts_with("-n") || arg == "--lines" || arg == "--bytes" {
            if arg == "--lines" || arg == "--bytes" {
                i += 1;
            }
            i += 1;
            continue;
        }
        if is_flag(arg) {
            i += 1;
            continue;
        }
        files.push(arg);
        i += 1;
    }
    match last_file(&files) {
        Some(path) => Classified::Intent(ShellIntent::new(ShellVerb::Read, Some(path))),
        None => Classified::Noise,
    }
}

fn sed_intent(argv: &[String]) -> Classified {
    if argv.iter().any(|arg| is_sed_in_place(arg)) {
        let args: Vec<&str> = argv[1..].iter().map(String::as_str).collect();
        return match last_source_file(&args) {
            Some(path) => Classified::Intent(ShellIntent::new(ShellVerb::Edit, Some(path))),
            None => Classified::Opaque,
        };
    }
    let mut script: Option<&str> = None;
    let mut files: Vec<&str> = Vec::new();
    let mut i = 1;
    while i < argv.len() {
        let arg = argv[i].as_str();
        if arg == "--" {
            files.extend(argv[i + 1..].iter().map(String::as_str));
            break;
        }
        if arg == "-e" || arg == "--expression" {
            script = argv.get(i + 1).map(String::as_str);
            i += 2;
            continue;
        }
        if arg == "-f" || arg == "--file" {
            return Classified::Opaque;
        }
        if matches!(
            arg,
            "-n" | "--quiet"
                | "--silent"
                | "-E"
                | "-r"
                | "--regexp-extended"
                | "-l"
                | "--line-length"
        ) || is_flag(arg)
        {
            i += 1;
            continue;
        }
        if script.is_none_or(str::is_empty) {
            script = Some(arg);
        } else {
            files.push(arg);
        }
        i += 1;
    }
    let Some(path) = last_file(&files) else {
        return Classified::Noise;
    };
    Classified::Intent(ShellIntent {
        verb: ShellVerb::Read,
        path: Some(path),
        query: None,
        start_line: sed_start_line(script),
    })
}

fn tee_intent(argv: &[String]) -> Classified {
    let append = argv.iter().any(|arg| arg == "-a" || arg == "--append");
    let files = positional_args(argv, &[]);
    let Some(path) = last_file(&files) else {
        return Classified::Opaque;
    };
    let verb = if append {
        ShellVerb::Edit
    } else {
        ShellVerb::Write
    };
    Classified::Intent(ShellIntent::new(verb, Some(path)))
}

fn grep_intent(argv: &[String]) -> Classified {
    let mut query: Option<&str> = None;
    let mut files: Vec<&str> = Vec::new();
    let unset = |query: Option<&str>| query.is_none_or(str::is_empty);
    let mut i = 1;
    while i < argv.len() {
        let arg = argv[i].as_str();
        if arg == "--" {
            let rest = &argv[i + 1..];
            match rest.first() {
                Some(first) if unset(query) && !first.is_empty() => {
                    query = Some(first);
                    files.extend(rest[1..].iter().map(String::as_str));
                }
                _ => files.extend(rest.iter().map(String::as_str)),
            }
            break;
        }
        if arg == "-e" || arg == "--regexp" {
            query = argv.get(i + 1).map(String::as_str);
            i += 2;
            continue;
        }
        if is_grep_value_flag(arg) {
            i += 2;
            continue;
        }
        if is_flag(arg) {
            if arg.starts_with("-e") && arg.len() > 2 {
                query = Some(&arg[2..]);
            }
            i += 1;
            continue;
        }
        if unset(query) {
            query = Some(arg);
        } else {
            files.push(arg);
        }
        i += 1;
    }
    let Some(query) = query.filter(|query| !query.is_empty()) else {
        return Classified::Opaque;
    };
    Classified::Intent(ShellIntent {
        verb: ShellVerb::Find,
        path: last_file(&files),
        query: Some(query.to_string()),
        start_line: None,
    })
}

fn find_intent(argv: &[String]) -> Classified {
    let mut query: Option<&str> = None;
    let mut path: Option<String> = None;
    let mut i = 1;
    while i < argv.len() {
        let arg = argv[i].as_str();
        if matches!(arg, "-name" | "-iname" | "-path" | "-ipath") {
            query = argv.get(i + 1).map(String::as_str);
            i += 2;
            continue;
        }
        if arg.starts_with('-') {
            if is_find_value_flag(arg) {
                i += 1;
            }
            i += 1;
            continue;
        }
        if path.is_none() {
            path = tidy_path(Some(arg));
        }
        i += 1;
    }
    let Some(query) = query.filter(|query| !query.is_empty()) else {
        return Classified::Opaque;
    };
    Classified::Intent(ShellIntent {
        verb: ShellVerb::Find,
        path,
        query: Some(query.to_string()),
        start_line: None,
    })
}

fn list_intent(argv: &[String]) -> Classified {
    let files = positional_args(argv, &["--ignore", "-I", "--hide"]);
    match last_file(&files) {
        Some(path) => Classified::Intent(ShellIntent::new(ShellVerb::List, Some(path))),
        None => Classified::Noise,
    }
}

fn positional_args<'a>(argv: &'a [String], value_flags: &[&str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut i = 1;
    while i < argv.len() {
        let arg = argv[i].as_str();
        if arg == "--" {
            out.extend(argv[i + 1..].iter().map(String::as_str));
            break;
        }
        if is_flag(arg) {
            if value_flags.contains(&arg) {
                i += 1;
            }
            i += 1;
            continue;
        }
        out.push(arg);
        i += 1;
    }
    out
}

fn last_file(files: &[&str]) -> Option<String> {
    files.iter().rev().find_map(|file| tidy_path(Some(file)))
}

fn last_source_file(args: &[&str]) -> Option<String> {
    args.iter().rev().find_map(|arg| {
        if arg.is_empty() || *arg == "-" || is_flag(arg) || is_sed_script(arg) {
            return None;
        }
        tidy_path(Some(arg))
    })
}

/// `/^\d+(,\d+)?[spd]$/` or `/^s([^A-Za-z0-9]).+\1/`.
fn is_sed_script(value: &str) -> bool {
    if js_regex!(r"^[0-9]+(?:,[0-9]+)?[spd]$").is_match(value) {
        return true;
    }
    let mut chars = value.chars();
    if chars.next() != Some('s') {
        return false;
    }
    let Some(delimiter) = chars.next().filter(|c| !c.is_ascii_alphanumeric()) else {
        return false;
    };
    // `.+` takes at least one character before the closing delimiter and never
    // crosses a line terminator.
    for (index, c) in chars.enumerate() {
        if index >= 1 && c == delimiter {
            return true;
        }
        if js::is_line_terminator(c) {
            return false;
        }
    }
    false
}

fn tidy_path(value: Option<&str>) -> Option<String> {
    let value = value?;
    if matches!(value, "" | "-" | "/dev/stdin" | "/dev/stdout") {
        return None;
    }
    if value.contains('>') || value.contains('<') {
        return None;
    }
    let slashed = value.replace('\\', "/");
    let trimmed = slashed.trim_end_matches('/');
    if matches!(trimmed, "" | "." | "./" | "/dev/null") {
        return None;
    }
    if trimmed.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(trimmed.to_string())
}

fn bin_name(token: &str) -> String {
    let slashed = token.replace('\\', "/");
    let base = slashed.rsplit('/').next().unwrap_or(&slashed);
    base.to_lowercase()
}

fn is_sed_in_place(arg: &str) -> bool {
    arg == "--in-place" || arg == "-i" || arg.starts_with("-i") || arg.starts_with("--in-place=")
}

fn sed_start_line(script: Option<&str>) -> Option<i64> {
    let found = js_regex!(r"^([0-9]+)(?:,[0-9]+)?p$").captures(script?)?;
    let line = found[1].parse::<f64>().ok()? as i64;
    (line > 0).then_some(line)
}

/// Walks `command` outside quotes and backslash escapes, the scan that
/// `looksUnsafe`, `extractWriteRedirect`, and `splitTopLevel` share. `visit`
/// gets each unquoted byte index and returns how far to move.
fn scan_unquoted(command: &str, mut visit: impl FnMut(usize) -> Step) {
    let bytes = command.as_bytes();
    let mut quote: Option<u8> = None;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(open) = quote {
            if open == b'"' && c == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if c == open {
                quote = None;
            }
            i += 1;
            continue;
        }
        if c == b'\'' || c == b'"' {
            quote = Some(c);
            i += 1;
            continue;
        }
        if c == b'\\' && i + 1 < bytes.len() {
            i += 2;
            continue;
        }
        match visit(i) {
            Step::Next => i += 1,
            Step::To(next) => i = next + 1,
            Step::Stop => return,
        }
    }
    if quote.is_some() {
        visit(usize::MAX);
    }
}

enum Step {
    Next,
    /// Continue after this index.
    To(usize),
    Stop,
}

fn looks_unsafe(command: &str) -> bool {
    let bytes = command.as_bytes();
    let mut unsafe_found = false;
    scan_unquoted(command, |i| {
        if i == usize::MAX {
            // The command ended inside a quote.
            unsafe_found = true;
            return Step::Stop;
        }
        let c = bytes[i];
        if c == b'`' || (c == b'$' && matches!(bytes.get(i + 1), Some(b'(' | b'{'))) {
            unsafe_found = true;
            return Step::Stop;
        }
        Step::Next
    });
    unsafe_found
}

struct WriteRedirect {
    path: String,
    append: bool,
}

fn extract_write_redirect(command: &str) -> Option<WriteRedirect> {
    let bytes = command.as_bytes();
    let mut found = None;
    scan_unquoted(command, |i| {
        if i == usize::MAX || bytes[i] != b'>' {
            return Step::Next;
        }
        let mut append = false;
        let mut j = i + 1;
        if bytes.get(j) == Some(&b'>') {
            append = true;
            j += 1;
        }
        while matches!(bytes.get(j), Some(b' ' | b'\t')) {
            j += 1;
        }
        if bytes.get(j) == Some(&b'&') {
            return Step::Next;
        }
        let target = read_unquoted_token(command, j);
        if target == "/dev/null" || target == "-" {
            return Step::Next;
        }
        if let Some(path) = tidy_path(Some(target)) {
            found = Some(WriteRedirect { path, append });
        }
        Step::To(i.max(j + target.len().max(1) - 1))
    });
    found
}

fn read_unquoted_token(text: &str, start: usize) -> &str {
    let rest = &text[start..];
    let end = rest
        .find(|c: char| js::is_space(c) || matches!(c, '|' | '&' | ';' | '<' | '>'))
        .unwrap_or(rest.len());
    &rest[..end]
}

fn chain_sep(text: &str, i: usize) -> usize {
    let rest = &text.as_bytes()[i..];
    if rest.starts_with(b"&&") || rest.starts_with(b"||") || rest.starts_with(b"\r\n") {
        return 2;
    }
    match rest.first() {
        Some(b'\n' | b'\r' | b';') => 1,
        _ => 0,
    }
}

fn pipe_sep(text: &str, i: usize) -> usize {
    let bytes = text.as_bytes();
    if bytes[i] == b'|' && bytes.get(i + 1) != Some(&b'|') {
        1
    } else {
        0
    }
}

fn split_top_level(command: &str, sep_at: fn(&str, usize) -> usize) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    scan_unquoted(command, |i| {
        if i == usize::MAX {
            return Step::Stop;
        }
        let len = sep_at(command, i);
        if len == 0 {
            return Step::Next;
        }
        let part = js::trim(&command[start..i]);
        if !part.is_empty() {
            parts.push(part);
        }
        let last = i + len - 1;
        start = last + 1;
        Step::To(last)
    });
    let tail = js::trim(&command[start..]);
    if !tail.is_empty() {
        parts.push(tail);
    }
    parts
}

struct ShellToken {
    value: String,
    start: usize,
    end: usize,
}

fn is_token_separator(c: u8) -> bool {
    matches!(
        c,
        b' ' | b'\t' | b'|' | b'&' | b';' | b'<' | b'>' | b'(' | b')'
    )
}

/// The character at byte `index`, which is always a character boundary here.
fn char_at(text: &str, index: usize) -> char {
    text[index..].chars().next().unwrap_or('\0')
}

fn tokenize(stage: &str) -> Option<Vec<ShellToken>> {
    let bytes = stage.as_bytes();
    let n = bytes.len();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < n {
        // Treat redirects (`2>&1`) as separators so `&` cannot stall the scan.
        while i < n && is_token_separator(bytes[i]) {
            i += 1;
        }
        if i >= n || bytes[i] == b'#' {
            break;
        }
        let start = i;
        let mut token = String::new();
        while i < n && !is_token_separator(bytes[i]) {
            let c = bytes[i];
            if c == b'\'' {
                let end = i + 1 + stage[i + 1..].find('\'')?;
                token.push_str(&stage[i + 1..end]);
                i = end + 1;
                continue;
            }
            if c == b'"' {
                i += 1;
                while i < n && bytes[i] != b'"' {
                    if bytes[i] == b'\\' && i + 1 < n {
                        let escaped = char_at(stage, i + 1);
                        token.push(escaped);
                        i += 1 + escaped.len_utf8();
                        continue;
                    }
                    let plain = char_at(stage, i);
                    token.push(plain);
                    i += plain.len_utf8();
                }
                if i >= n {
                    return None;
                }
                i += 1;
                continue;
            }
            if c == b'\\' && i + 1 < n {
                let escaped = char_at(stage, i + 1);
                token.push(escaped);
                i += 1 + escaped.len_utf8();
                continue;
            }
            let plain = char_at(stage, i);
            token.push(plain);
            i += plain.len_utf8();
        }
        if !token.is_empty() {
            tokens.push(ShellToken {
                value: token,
                start,
                end: i,
            });
        }
        if i <= start {
            i += 1;
        }
    }
    Some(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent(
        verb: ShellVerb,
        path: Option<&str>,
        query: Option<&str>,
        line: Option<i64>,
    ) -> ShellIntent {
        ShellIntent {
            verb,
            path: path.map(str::to_string),
            query: query.map(str::to_string),
            start_line: line,
        }
    }

    fn read(path: &str) -> Option<ShellIntent> {
        Some(intent(ShellVerb::Read, Some(path), None, None))
    }

    // inferShellIntent
    #[test]
    fn reads_a_file_from_cat_head_sed_n() {
        assert_eq!(infer_shell_intent("cat package.json"), read("package.json"));
        assert_eq!(
            infer_shell_intent(
                r#"cd /Users/nikolaypetkov/code/agent-terminal && ls && echo "---" && cat package.json | head -60"#
            ),
            read("package.json")
        );
        assert_eq!(
            infer_shell_intent(
                r#"ls src/lib/harness/ && echo "=== preview ===" && sed -n '1,140p' src/lib/harness/preview.ts"#
            ),
            Some(intent(
                ShellVerb::Read,
                Some("src/lib/harness/preview.ts"),
                None,
                Some(1)
            ))
        );
        assert_eq!(
            infer_shell_intent("sed -n '713,1200p' src/surfaces/AgentTranscript.tsx"),
            Some(intent(
                ShellVerb::Read,
                Some("src/surfaces/AgentTranscript.tsx"),
                None,
                Some(713)
            ))
        );
        assert_eq!(
            infer_shell_intent(
                r#"/bin/zsh -lc "nl -ba src/lib/orchestration.ts | sed -n '1,260p'""#
            ),
            read("src/lib/orchestration.ts")
        );
        assert_eq!(
            infer_shell_intent(
                "/bin/zsh -lc \"sed -n '1,260p' src/surfaces/transcriptActivity.ts\nsed -n '880,980p' src/surfaces/transcriptActivity.test.ts\nsed -n '960,1060p' src/index.css\""
            ),
            Some(intent(
                ShellVerb::Read,
                Some("src/index.css"),
                None,
                Some(960)
            ))
        );
    }

    #[test]
    fn treats_grep_rg_as_find() {
        assert_eq!(
            infer_shell_intent(
                r#"grep -n "^function \|^const .* = memo" src/surfaces/AgentTranscript.tsx"#
            ),
            Some(intent(
                ShellVerb::Find,
                Some("src/surfaces/AgentTranscript.tsx"),
                Some("^function |^const .* = memo"),
                None
            ))
        );
        assert_eq!(
            infer_shell_intent("rg -n isReadTool src/lib/harness"),
            Some(intent(
                ShellVerb::Find,
                Some("src/lib/harness"),
                Some("isReadTool"),
                None
            ))
        );
        assert_eq!(
            infer_shell_intent(r#"/bin/bash -lc "rg -n 'submissionError|hydrate' src/lib""#),
            Some(intent(
                ShellVerb::Find,
                Some("src/lib"),
                Some("submissionError|hydrate"),
                None
            ))
        );
        assert_eq!(
            infer_shell_intent(
                "/bin/zsh -lc \"rg -n \\\"function upsertTool\\\" src/lib/harness/apply.ts && sed -n '520,620p' src/lib/harness/apply.ts\nrg -n \\\"mapApprovalRequest\\\" src/lib/harness/codexProtocol.test.ts | head -n 60\nsed -n '560,640p' src/lib/harness/codexProtocol.test.ts\""
            ),
            Some(intent(
                ShellVerb::Read,
                Some("src/lib/harness/codexProtocol.test.ts"),
                None,
                Some(560)
            ))
        );
        assert_eq!(
            infer_shell_intent("find src -name '*.ts'"),
            Some(intent(ShellVerb::Find, Some("src"), Some("*.ts"), None))
        );
    }

    #[test]
    fn lists_a_directory_when_that_is_all_the_command_does() {
        assert_eq!(
            infer_shell_intent("ls src/lib/harness/"),
            Some(intent(ShellVerb::List, Some("src/lib/harness"), None, None))
        );
        assert_eq!(infer_shell_intent("ls -la"), None);
        assert_eq!(
            infer_shell_intent("rg --files | sed -n '1,240p'"),
            Some(intent(ShellVerb::Find, None, Some("files"), None))
        );
    }

    #[test]
    fn leaves_real_shell_as_the_command() {
        for command in [
            "git status -s",
            "/bin/zsh -lc 'git status -s'",
            "git diff src/surfaces/AgentTranscript.tsx",
            "npm test",
            "cat file && python script.py",
            "cat $(echo foo)",
            "python3 - <<'PY'",
            "rm src/hooks/useActivityTicker.ts && python3 - <<'PY'",
        ] {
            assert_eq!(infer_shell_intent(command), None, "{command}");
        }
    }

    #[test]
    fn treats_file_mutating_bash_as_edit_write() {
        assert_eq!(
            infer_shell_intent("sed -i 's/a/b/' src/app.ts"),
            Some(intent(ShellVerb::Edit, Some("src/app.ts"), None, None))
        );
        assert_eq!(
            infer_shell_intent(
                "sed -i '' 's/zen-ticker-live/zen-tool-spin/' src/surfaces/AgentTranscript.tsx"
            ),
            Some(intent(
                ShellVerb::Edit,
                Some("src/surfaces/AgentTranscript.tsx"),
                None,
                None
            ))
        );
        assert_eq!(
            infer_shell_intent("cat >> src/surfaces/transcriptActivity.ts <<'TS'"),
            Some(intent(
                ShellVerb::Edit,
                Some("src/surfaces/transcriptActivity.ts"),
                None,
                None
            ))
        );
        assert_eq!(
            infer_shell_intent("cat package.json > out.json"),
            Some(intent(ShellVerb::Write, Some("out.json"), None, None))
        );
        assert_eq!(
            infer_shell_intent("tee src/index.css"),
            Some(intent(ShellVerb::Write, Some("src/index.css"), None, None))
        );
    }

    #[test]
    fn does_not_hang_on_fd_redirects_like_2_and_1() {
        let find = Some(intent(
            ShellVerb::Find,
            Some("src/app.ts"),
            Some("foo"),
            None,
        ));
        assert_eq!(infer_shell_intent("grep foo src/app.ts 2>&1"), find);
        assert_eq!(
            infer_shell_intent("cat package.json 2>&1"),
            read("package.json")
        );
        assert_eq!(infer_shell_intent("git status 2>&1"), None);
        assert_eq!(infer_shell_intent("grep foo src/app.ts 2>/dev/null"), find);
    }

    #[test]
    fn skips_already_labelled_rows_and_huge_scripts() {
        assert_eq!(infer_shell_intent("Read src/lib/appearance.ts"), None);
        assert_eq!(
            infer_shell_intent(&format!("cat {}.ts", "a".repeat(3000))),
            None
        );
    }

    // formatShellIntent
    #[test]
    fn prefers_a_display_path_passed_in_from_the_transcript() {
        assert_eq!(
            format_shell_intent(
                &intent(
                    ShellVerb::Read,
                    Some("/Users/me/proj/src/app.ts"),
                    None,
                    None
                ),
                Some("src/app.ts"),
                None
            )
            .as_deref(),
            Some("Read src/app.ts")
        );
    }

    // unwrapShellCommand
    #[test]
    fn unwraps_posix_shells_without_including_trailing_shell_arguments() {
        assert_eq!(
            unwrap_shell_command(r#"/bin/zsh -lc "npm test -- --run app.test.ts""#),
            "npm test -- --run app.test.ts"
        );
        assert_eq!(
            unwrap_shell_command(r#"/bin/zsh -lc "rg -n \"foo\" src" ignored"#),
            r#"rg -n "foo" src"#
        );
    }

    #[test]
    fn unwraps_powershell_command_remainders() {
        assert_eq!(
            unwrap_shell_command(
                r#""C:\Program Files\PowerShell\7\pwsh.exe" -NoLogo -NoProfile -Command 'rg -n foo src'"#
            ),
            "rg -n foo src"
        );
        assert_eq!(
            unwrap_shell_command(
                "powershell.exe -ExecutionPolicy Bypass -Command Get-Content package.json"
            ),
            "Get-Content package.json"
        );
        assert_eq!(
            unwrap_shell_command("pwsh -c Get-Content package.json"),
            "Get-Content package.json"
        );
        assert_eq!(
            unwrap_shell_command(r#"pwsh "-Command" "Get-Content package.json""#),
            "Get-Content package.json"
        );
        assert_eq!(
            unwrap_shell_command(r#"pwsh -Command "Get-Content".ps1"#),
            r#""Get-Content".ps1"#
        );
        assert_eq!(
            unwrap_shell_command("pwsh -Command 'Get-Date' '-Format' 'yyyy-MM-dd'"),
            "'Get-Date' '-Format' 'yyyy-MM-dd'"
        );
    }

    #[test]
    fn unwraps_cmd_command_remainders() {
        assert_eq!(
            unwrap_shell_command(r#"cmd.exe /d /s /c "npm test""#),
            "npm test"
        );
    }

    #[test]
    fn stops_scanning_powershell_launcher_options_at_file() {
        for command in [
            "pwsh -File script.ps1 -Mode -Command build",
            "pwsh -f script.ps1 -Mode -c build",
            r#"pwsh "-File" script.ps1 "-Command" build"#,
        ] {
            assert_eq!(unwrap_shell_command(command), command);
        }
    }

    #[test]
    fn leaves_ordinary_and_incomplete_commands_unchanged() {
        assert_eq!(
            unwrap_shell_command("git status --short"),
            "git status --short"
        );
        assert_eq!(
            unwrap_shell_command("pwsh -Command 'npm test"),
            "pwsh -Command 'npm test"
        );
    }

    #[test]
    fn rewrites_stored_readable_titles() {
        assert_eq!(
            rewrite_readable_title("read src/a.ts", Some("a.ts"), None).as_deref(),
            Some("Read a.ts")
        );
        assert_eq!(
            rewrite_readable_title("FIND foo", None, Some("bar")).as_deref(),
            Some("Find bar")
        );
        assert_eq!(rewrite_readable_title("Read\nfoo\nbar", None, None), None);
        assert!(is_sed_script("s/a/b/"));
        assert!(is_sed_script("12,14p"));
        assert!(!is_sed_script("s//"));
        assert!(!is_sed_script("sab"));
    }
}
