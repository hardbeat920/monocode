//! Port of src/integrations/harness/providers/pi/piFlavor.ts.
//!
//! omp (oh-my-pi) is a fork of Pi and speaks the same `--mode rpc` NDJSON
//! protocol, so both harnesses run on one adapter core. A flavor carries the
//! few details that differ between the two CLIs.
//!
//! The TypeScript flavor also held `resolveBinary`. Here the family keeps the
//! resolver in its per-flavor state, so tests can replace it.

use monocode_core::HarnessId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PiFlavor {
    pub id: HarnessId,
    /// Name used in error messages and debug logs.
    pub label: &'static str,
    /// Flag that resumes a stored session by id.
    pub resume_flag: &'static str,
    /// Flags that strip tools, skills, and project context for one-shot jobs.
    pub isolate_flags: &'static [&'static str],
    /// Read-only tools exposed while the shared composer is in Plan mode.
    pub plan_tools: &'static [&'static str],
    /// Child id for the shared catalog probe.
    pub probe_child_id: &'static str,
    /// Child id for the shared one-shot text generator.
    pub text_child_id: &'static str,
}

impl PiFlavor {
    pub fn is_omp(&self) -> bool {
        self.id == HarnessId::Omp
    }

    pub fn is_pi(&self) -> bool {
        self.id == HarnessId::Pi
    }
}

/// `PI_FLAVOR`.
pub const PI_FLAVOR: PiFlavor = PiFlavor {
    id: HarnessId::Pi,
    label: "Pi",
    resume_flag: "--session",
    isolate_flags: &["--no-tools", "--no-skills", "--no-context-files"],
    plan_tools: &["read", "grep", "find", "ls"],
    probe_child_id: "monocode-pi-probe",
    text_child_id: "monocode-pi-text",
};

/// `OMP_FLAVOR`. omp renamed two of Pi's flags: sessions resume through
/// `--resume` (Pi uses `--session`), and `--no-rules` strips project context
/// (Pi uses `--no-context-files`). Verified against omp 18.0.6 `--help`.
pub const OMP_FLAVOR: PiFlavor = PiFlavor {
    id: HarnessId::Omp,
    label: "omp",
    resume_flag: "--resume",
    isolate_flags: &["--no-tools", "--no-skills", "--no-rules"],
    plan_tools: &["read", "grep", "glob", "lsp"],
    probe_child_id: "monocode-omp-probe",
    text_child_id: "monocode-omp-text",
};
