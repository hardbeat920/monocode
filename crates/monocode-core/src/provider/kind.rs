//! Provider identity is shared by discovery, persistence, pickers and transports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Codex,
    Claude,
    Cursor,
    Grok,
    OpenCode,
    Antigravity,
    Pi,
    Omp,
    Fx,
    Hermes,
    Devin,
}
impl Kind {
    pub const ALL: [Self; 11] = [
        Self::Claude,
        Self::Codex,
        Self::Cursor,
        Self::Grok,
        Self::OpenCode,
        Self::Pi,
        Self::Omp,
        Self::Fx,
        Self::Hermes,
        Self::Antigravity,
        Self::Devin,
    ];
    pub fn id(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Cursor => "cursor",
            Self::Grok => "grok",
            Self::OpenCode => "opencode",
            Self::Antigravity => "antigravity",
            Self::Pi => "pi",
            Self::Omp => "omp",
            Self::Fx => "fx",
            Self::Hermes => "hermes",
            Self::Devin => "devin",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude Code",
            Self::Cursor => "Cursor",
            Self::Grok => "Grok Build",
            Self::OpenCode => "OpenCode",
            Self::Antigravity => "Antigravity",
            Self::Pi => "Pi",
            Self::Omp => "omp",
            Self::Fx => "fx",
            Self::Hermes => "Hermes Agent",
            Self::Devin => "Devin",
        }
    }
    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.id() == id)
    }
    pub fn from_id(id: &str) -> Self {
        Self::parse(id).unwrap_or(Self::Codex)
    }
    pub fn binary_name(self) -> &'static str {
        match self {
            Self::Cursor => "cursor-agent",
            Self::Antigravity => "agy_acp_server.par",
            _ => self.id(),
        }
    }
    pub(super) fn install_dirs(self) -> &'static [&'static str] {
        match self {
            Self::Grok => &[".grok/bin"],
            Self::OpenCode => &[".opencode/bin"],
            Self::Fx => &[".fx/bin"],
            Self::Antigravity => &[".local/share/agy-acp"],
            Self::Hermes => &[
                ".hermes/hermes-agent/venv/bin",
                ".hermes/hermes-agent/.venv/bin",
            ],
            _ => &[],
        }
    }
    pub fn is_acp(self) -> bool {
        !matches!(self, Self::Codex | Self::Claude | Self::Pi | Self::Omp)
    }
    pub fn is_pi(self) -> bool {
        matches!(self, Self::Pi | Self::Omp)
    }
    pub fn subscription_usage(self) -> bool {
        matches!(self, Self::Codex | Self::Claude)
    }
    /// The name shown when the binary is missing, matching the React harness map.
    fn install_name(self) -> &'static str {
        match self {
            Self::Codex => "Codex CLI",
            Self::Claude => "Claude Code CLI",
            Self::Cursor => "Cursor CLI",
            Self::Grok => "Grok Build CLI",
            Self::OpenCode => "OpenCode CLI",
            Self::Antigravity => "Antigravity ACP server (agy_acp_server.par)",
            Self::Pi => "Pi CLI",
            Self::Omp => "omp CLI",
            Self::Fx => "fx CLI",
            Self::Hermes => "Hermes Agent CLI",
            Self::Devin => "Devin CLI",
        }
    }
    /// The install command React shows in its hint, when it has one.
    fn install_command(self) -> Option<&'static str> {
        match self {
            Self::Grok => Some("curl -fsSL https://x.ai/cli/install.sh | bash"),
            Self::Pi => Some("npm i -g @earendil-works/pi-coding-agent"),
            Self::Omp => Some("curl -fsSL https://omp.sh/install | sh"),
            Self::Fx => Some("curl -fsSL https://fx.sh/setup.sh | bash"),
            Self::Hermes => {
                Some("Install from hermes-agent.nousresearch.com, then run hermes model")
            }
            #[cfg(windows)]
            Self::Devin => Some("irm https://static.devin.ai/cli/setup.ps1 | iex"),
            #[cfg(not(windows))]
            Self::Devin => Some("curl -fsSL https://cli.devin.ai/install.sh | bash"),
            Self::Codex | Self::Claude | Self::Cursor | Self::OpenCode | Self::Antigravity => None,
        }
    }
    /// The same "not found" copy React uses. React only checks that a binary
    /// exists, so the hint never mentions signing in.
    pub fn unavailable_hint(self) -> String {
        let name = self.install_name();
        match self.install_command() {
            Some(install) => format!(
                "{name} not found (`{install}`). Install it, or restart MonoCode if it is already installed."
            ),
            None => format!(
                "{name} not found. Install it, or restart MonoCode if it is already installed."
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_matches_the_react_harnesses() {
        let source = include_str!("../../../../src/integrations/harness/core/register.ts");
        for kind in Kind::ALL {
            assert_eq!(Kind::parse(kind.id()), Some(kind));
            assert!(source.contains(&format!("../providers/{}/", kind.id())));
        }
        assert_eq!(Kind::parse("unknown"), None);
        assert_eq!(Kind::Cursor.binary_name(), "cursor-agent");
    }
    #[test]
    fn unavailable_hint_matches_the_react_copy() {
        assert_eq!(
            Kind::Antigravity.unavailable_hint(),
            "Antigravity ACP server (agy_acp_server.par) not found. Install it, or restart MonoCode if it is already installed."
        );
        assert_eq!(
            Kind::Pi.unavailable_hint(),
            "Pi CLI not found (`npm i -g @earendil-works/pi-coding-agent`). Install it, or restart MonoCode if it is already installed."
        );
        assert_eq!(
            Kind::Codex.unavailable_hint(),
            "Codex CLI not found. Install it, or restart MonoCode if it is already installed."
        );
    }
    #[test]
    fn hints_match_the_react_source() {
        let source = include_str!("../../../../src/integrations/harness/core/availability.ts");
        for kind in Kind::ALL {
            assert!(
                source.contains(&format!("\"{}\"", kind.install_name())),
                "React is missing the install name for {}",
                kind.id()
            );
            if let Some(install) = kind.install_command() {
                assert!(
                    source.contains(install),
                    "React is missing the install command for {}",
                    kind.id()
                );
            }
        }
        assert!(source.contains(
            "not found${how}. Install it, or restart MonoCode if it is already installed."
        ));
    }
}
