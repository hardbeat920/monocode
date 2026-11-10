/// Replace JSON comments with whitespace without changing quoted strings.
/// Reject unterminated block comments instead of silently accepting a partial
/// config. JSON syntax and schema validation remain the caller's responsibility.
pub(crate) fn strip_jsonc_comments(raw: &str) -> Option<String> {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match (ch, chars.peek().copied()) {
            ('"', _) => {
                in_string = true;
                out.push(ch);
            }
            ('/', Some('/')) => {
                let _ = chars.next();
                out.push(' ');
                for next in chars.by_ref() {
                    if matches!(next, '\n' | '\r') {
                        out.push(next);
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                let _ = chars.next();
                out.push(' ');
                let mut closed = false;
                while let Some(next) = chars.next() {
                    if next == '\n' {
                        out.push('\n');
                    }
                    if next == '*' && chars.next_if_eq(&'/').is_some() {
                        closed = true;
                        break;
                    }
                }
                if !closed {
                    return None;
                }
            }
            _ => out.push(ch),
        }
    }
    Some(out)
}
