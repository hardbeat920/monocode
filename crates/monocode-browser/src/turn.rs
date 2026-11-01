//! What a user's turn does about the browser: the `/browser` command, and
//! whether a turn without it carries the browser tools, grants the thread
//! first or asks on the first action.
//!
//! The Tauri app's frontend has the same rules in
//! `src/features/sessions/model/browserCommand.ts`; Monocode Native uses
//! these. Keep the two in step until the Tauri app goes.

use crate::protocol::{AccessMode, BrowserState, SessionAccess};

/// The command's name, as the composer lists it.
pub const COMMAND: &str = "browser";
/// The composer's description of `/browser`.
pub const COMMAND_DESCRIPTION: &str = "Let this thread open and use its own tabs in your browser.";

/// The line an "ask" turn adds to the `<monocode_browser>` block.
pub const ASK_NOTE: &str = "This thread has no browser access yet. Your first browser action asks the user in MonoCode and waits for the answer. If it returns not_granted, continue without the browser and do not try it again in this turn.";

/// The status line when the switch is off and the user typed `/browser`.
pub const OFF_NOTICE: &str =
    "Browser access is off for all agents. Turn it on in Settings → Browser, then try again.";

/// `/browser` at the start of a prompt, or right after a leading
/// `/operator`, `/mono` or `/monocode`: the prompt without it (any such
/// prefix kept, so its own parser still sees it), or `None`.
pub fn consume_command(text: &str) -> Option<String> {
    let trimmed = text.trim_start();
    let (prefix, rest) = match leading_word(trimmed) {
        Some((word, rest))
            if ["/operator", "/mono", "/monocode"]
                .iter()
                .any(|prefix| word.eq_ignore_ascii_case(prefix)) =>
        {
            let rest = rest.trim_start();
            let prefix_len = trimmed.len() - rest.len();
            (&trimmed[..prefix_len], rest)
        }
        _ => ("", trimmed),
    };
    let (word, after) = leading_word(rest)?;
    if !word.eq_ignore_ascii_case("/browser") {
        return None;
    }
    Some(format!("{prefix}{}", after.trim_start()))
}

/// The first whitespace-separated word and what follows it.
fn leading_word(text: &str) -> Option<(&str, &str)> {
    if text.is_empty() {
        return None;
    }
    let end = text.find(char::is_whitespace).unwrap_or(text.len());
    Some((&text[..end], &text[end..]))
}

/// The grant `/browser` sets, or `None` when it is already right. Alone it
/// switches; with a prompt it only turns access on.
pub fn grant_change(granted: bool, bare: bool) -> Option<bool> {
    if bare {
        return Some(!granted);
    }
    (!granted).then_some(true)
}

/// Status line after `/browser` alone switched the grant.
pub fn grant_status(enabled: bool) -> &'static str {
    if enabled {
        "Browser access is on for this thread. The agent opens its own tabs; an agent that is already running may need its next turn or a restart to see the browser tools."
    } else {
        "Browser access is off for this thread. Its tabs stay open, and the agent asks you before it uses the browser again."
    }
}

/// What a turn does about the browser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnAccess {
    /// The thread has access; the turn carries the browser tools.
    Granted,
    /// "Always on", or a thread with "Full access": grant the thread first,
    /// then carry the tools.
    Grant,
    /// Carry the tools without a grant; the first browser action asks the
    /// user in the thread and waits for the answer.
    Ask,
}

/// The session's row in `state`.
pub fn session<'a>(state: &'a BrowserState, session: &str) -> Option<&'a SessionAccess> {
    state
        .sessions
        .iter()
        .find(|entry| entry.session_id == session)
}

/// Whether the session has access (`/browser`, Allow or Always on); false
/// while the switch is off.
pub fn granted(state: &BrowserState, session_id: &str) -> bool {
    state.toggle_on && session(state, session_id).is_some_and(|entry| entry.granted)
}

/// An agent of the session waits for the user's Allow.
pub fn request_pending(state: &BrowserState, session_id: &str) -> bool {
    state.toggle_on && session(state, session_id).is_some_and(|entry| entry.requested)
}

/// What a turn without `/browser` does (`None`: no browser tools).
///
/// `eligible`: only a turn the user sends from a regular thread may get the
/// browser on its own; turns an agent sends through the app, orchestration
/// and managed turns use it only with a grant the thread already holds. A
/// thread the user turned off asks instead of being granted again, with
/// "Full access" too: turning it off is the more specific choice.
pub fn turn_access(
    state: &BrowserState,
    session_id: &str,
    eligible: bool,
    full_access: bool,
) -> Option<TurnAccess> {
    if !state.toggle_on {
        return None;
    }
    if granted(state, session_id) {
        return Some(TurnAccess::Granted);
    }
    if !eligible {
        return None;
    }
    let turned_off = session(state, session_id).is_some_and(|entry| entry.turned_off);
    let always = state.settings.access == AccessMode::Always || full_access;
    Some(if always && !turned_off {
        TurnAccess::Grant
    } else {
        TurnAccess::Ask
    })
}

/// Whether a session a Mono started takes the Mono's grant when the Mono
/// starts it or hands it work: only while the Mono holds one, and never for
/// a session the user turned off.
pub fn inherits_mono_grant(state: &BrowserState, mono_session: &str, session_id: &str) -> bool {
    granted(state, mono_session)
        && !granted(state, session_id)
        && !session(state, session_id).is_some_and(|entry| entry.turned_off)
}

/// Puts [`ASK_NOTE`] inside the `<monocode_browser>` block, or after it.
pub fn with_ask_note(block: &str) -> String {
    const CLOSE: &str = "</monocode_browser>";
    match block.rfind(CLOSE) {
        Some(end) => format!("{}\n{ASK_NOTE}\n{}", block[..end].trim_end(), &block[end..]),
        None => format!("{block}\n{ASK_NOTE}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::BrowserSettings;

    fn state(toggle_on: bool, access: AccessMode, entry: Option<SessionAccess>) -> BrowserState {
        let mut state = crate::BrowserHost::for_tests(BrowserSettings::default()).state();
        state.toggle_on = toggle_on;
        state.settings.access = access;
        state.sessions = entry.into_iter().collect();
        state
    }

    fn entry(granted: bool, turned_off: bool) -> SessionAccess {
        SessionAccess {
            session_id: "s1".into(),
            window: "main".into(),
            granted,
            turn_active: false,
            granted_since_ms: None,
            tabs: 0,
            requested: false,
            turned_off,
        }
    }

    #[test]
    fn the_command_is_found_alone_or_after_an_operator_prefix() {
        assert_eq!(consume_command("/browser"), Some(String::new()));
        assert_eq!(
            consume_command("  /BROWSER check the login page"),
            Some("check the login page".into())
        );
        assert_eq!(
            consume_command("/operator /browser open it"),
            Some("/operator open it".into())
        );
        assert_eq!(consume_command("/browsers"), None);
        assert_eq!(consume_command("use /browser"), None);
        assert_eq!(consume_command(""), None);
    }

    #[test]
    fn a_bare_command_switches_and_a_prompt_only_turns_access_on() {
        assert_eq!(grant_change(false, true), Some(true));
        assert_eq!(grant_change(true, true), Some(false));
        assert_eq!(grant_change(false, false), Some(true));
        assert_eq!(grant_change(true, false), None);
    }

    #[test]
    fn turns_ask_grant_or_carry_the_tools_as_the_settings_say() {
        let off = state(false, AccessMode::Always, Some(entry(true, false)));
        assert_eq!(turn_access(&off, "s1", true, true), None);
        let ask = state(true, AccessMode::Ask, None);
        assert_eq!(turn_access(&ask, "s1", true, false), Some(TurnAccess::Ask));
        assert_eq!(turn_access(&ask, "s1", false, false), None);
        assert_eq!(
            turn_access(&ask, "s1", true, true),
            Some(TurnAccess::Grant),
            "Full access grants"
        );
        let always = state(true, AccessMode::Always, None);
        assert_eq!(
            turn_access(&always, "s1", true, false),
            Some(TurnAccess::Grant)
        );
        let turned_off = state(true, AccessMode::Always, Some(entry(false, true)));
        assert_eq!(
            turn_access(&turned_off, "s1", true, true),
            Some(TurnAccess::Ask)
        );
        // A grant the thread holds counts even for an ineligible turn.
        let held = state(true, AccessMode::Ask, Some(entry(true, false)));
        assert_eq!(
            turn_access(&held, "s1", false, false),
            Some(TurnAccess::Granted)
        );
    }

    #[test]
    fn the_ask_note_goes_inside_the_block() {
        assert_eq!(
            with_ask_note("<monocode_browser>\nUse the tools.\n</monocode_browser>"),
            format!("<monocode_browser>\nUse the tools.\n{ASK_NOTE}\n</monocode_browser>")
        );
        assert_eq!(with_ask_note("text"), format!("text\n{ASK_NOTE}"));
    }
}
