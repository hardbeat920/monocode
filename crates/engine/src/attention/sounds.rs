//! Port of src/features/settings/model/sounds.ts: the sounds setting, which
//! cue plays which sound, the project policy for project cues, and the
//! one-shot announcements.
//!
//! The TypeScript played through cuelume. Here `SoundCues::play_cue`
//! decides, and the `AttentionPlatform` plays the sound (see
//! `sound_synth` and `platform`). `initSounds` and `applySoundEngine` have
//! no counterpart: the volume and the enabled flag are read on every cue.

use std::collections::HashSet;

use monocode_core::inbox::{
    InboxProvider, LinkedWorkItemUpdateCard, LinkedWorkItemUpdateStatus, WorkItemKind,
};
use monocode_settings::Kv;

use super::notification_preferences::{
    NotificationCategory, NotificationSubject, allows_project_notification,
};
use super::notification_projects::{NotificationWorkItem, inbox_notification_project};
use super::notifications::stored_flag;
use super::sound_synth::SoundName;

pub const SOUNDS_KEY: &str = "monocode.sounds";
pub const SOUNDS_ENABLED_AT_KEY: &str = "monocode.soundsEnabledAt";

/// `SOUNDS_DEFAULT`.
pub const SOUNDS_DEFAULT: bool = true;

/// `SOUNDS_VOLUME`: soft enough to sit in the background while a turn runs
/// in another app.
pub const SOUNDS_VOLUME: f64 = 0.55;

/// `SoundCue`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SoundCue {
    TurnFinished,
    InboxUnseen,
    LinkedActivity,
    UpdateAvailable,
    Switch,
    Copy,
}

impl SoundCue {
    /// `CUES`.
    pub const fn sound(self) -> SoundName {
        match self {
            SoundCue::TurnFinished => SoundName::Success,
            SoundCue::InboxUnseen => SoundName::Bloom,
            SoundCue::LinkedActivity => SoundName::Chime,
            SoundCue::UpdateAvailable => SoundName::Arrival,
            SoundCue::Switch => SoundName::Toggle,
            SoundCue::Copy => SoundName::Scan,
        }
    }

    /// `ProjectSoundCue`: cues that require their subject, so a new source
    /// cannot bypass project policy.
    pub const fn needs_subject(self) -> bool {
        matches!(
            self,
            SoundCue::TurnFinished | SoundCue::InboxUnseen | SoundCue::LinkedActivity
        )
    }
}

/// `loadSoundsEnabled`.
pub fn load_sounds_enabled(kv: &Kv) -> bool {
    stored_flag(kv.get_item(SOUNDS_KEY), SOUNDS_DEFAULT)
}

/// `saveSoundsEnabled`. Turning sounds back on stamps the time, so delayed
/// project activity from while they were off stays quiet.
pub fn save_sounds_enabled(kv: &Kv, value: bool, now: i64) {
    let resuming = value && !load_sounds_enabled(kv);
    kv.set_item(SOUNDS_KEY, if value { "1" } else { "0" });
    if resuming {
        kv.set_item(SOUNDS_ENABLED_AT_KEY, &now.to_string());
    }
}

/// The policy half of `playCue`: whether `cue` may play now.
pub fn cue_allowed(
    kv: &Kv,
    cue: SoundCue,
    subject: Option<&NotificationSubject>,
    now: i64,
) -> bool {
    debug_assert!(
        !cue.needs_subject() || subject.is_some(),
        "{cue:?} needs a notification subject"
    );
    if !load_sounds_enabled(kv) {
        return false;
    }
    if let Some(subject) = subject {
        if !allows_project_notification(kv, subject, now) {
            return false;
        }
        if let Some(occurred_at) = subject.occurred_at {
            // `Number(null)` is 0, and a bad value is NaN, which no time is below.
            let enabled_at = kv
                .get_item(SOUNDS_ENABLED_AT_KEY)
                .map(|raw| monocode_core::js::parse_number(&raw))
                .unwrap_or(Some(0.0));
            if enabled_at.is_some_and(|enabled_at| (occurred_at as f64) < enabled_at) {
                return false;
            }
        }
    }
    true
}

/// The module state of sounds.ts: which one-shot cues already fired.
#[derive(Debug, Default)]
pub struct SoundCues {
    announced_update: Option<String>,
    announced_linked_activities: HashSet<String>,
}

impl SoundCues {
    /// `announceLinkedActivity`: the subject and key for a linked item's new
    /// activity, or `None` when this session already announced it. The key
    /// survives notice unmounts when switching tabs.
    pub fn linked_activity_subject(
        &mut self,
        session_id: &str,
        card: Option<&LinkedWorkItemUpdateCard>,
    ) -> Option<NotificationSubject> {
        let card = card.filter(|card| card.status == LinkedWorkItemUpdateStatus::Ready)?;
        let kind = match card.kind {
            WorkItemKind::Pr => "pr",
            WorkItemKind::Issue => "issue",
        };
        let key = serde_json::json!([
            session_id,
            kind,
            card.repo.to_lowercase(),
            card.number,
            card.updated_at
        ])
        .to_string();
        if !self.announced_linked_activities.insert(key) {
            return None;
        }
        let project = inbox_notification_project(&NotificationWorkItem::new(
            InboxProvider::Github,
            &card.repo,
            &card.url,
        ));
        Some(NotificationSubject {
            project_id: project.id,
            category: if card.kind == WorkItemKind::Pr {
                NotificationCategory::PullRequests
            } else {
                NotificationCategory::Issues
            },
            occurred_at: Some(card.updated_at),
        })
    }

    /// `announceUpdateAvailable`: one cue per available version, including a
    /// later probe of the same build. Returns whether to play it.
    pub fn update_available(&mut self, version: Option<&str>) -> bool {
        let Some(version) = version.filter(|version| !version.is_empty()) else {
            self.announced_update = None;
            return false;
        };
        if self.announced_update.as_deref() == Some(version) {
            return false;
        }
        self.announced_update = Some(version.to_string());
        true
    }

    /// `resetSoundCues`.
    pub fn reset(&mut self) {
        self.announced_update = None;
        self.announced_linked_activities.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::notification_preferences::{
        Mute, PreferencePatch, update_notification_preferences,
    };
    use monocode_core::inbox::LinkedWorkItemActivityCounts;

    fn subject(occurred_at: Option<i64>) -> NotificationSubject {
        NotificationSubject {
            project_id: "local:/one".into(),
            category: NotificationCategory::AgentFinished,
            occurred_at,
        }
    }

    #[test]
    fn sounds_default_on_and_round_trip() {
        let kv = Kv::in_memory();
        assert!(load_sounds_enabled(&kv));
        save_sounds_enabled(&kv, false, 1);
        assert!(!load_sounds_enabled(&kv));
        assert!(!cue_allowed(&kv, SoundCue::Copy, None, 2));
        save_sounds_enabled(&kv, true, 5);
        assert_eq!(kv.get_item(SOUNDS_ENABLED_AT_KEY).as_deref(), Some("5"));
        assert!(cue_allowed(&kv, SoundCue::Copy, None, 6));
    }

    #[test]
    fn project_cues_follow_the_project_policy_and_skip_activity_from_while_sounds_were_off() {
        let kv = Kv::in_memory();
        assert!(cue_allowed(
            &kv,
            SoundCue::TurnFinished,
            Some(&subject(Some(10))),
            20
        ));
        save_sounds_enabled(&kv, false, 30);
        save_sounds_enabled(&kv, true, 40);
        assert!(!cue_allowed(
            &kv,
            SoundCue::TurnFinished,
            Some(&subject(Some(35))),
            50
        ));
        assert!(cue_allowed(
            &kv,
            SoundCue::TurnFinished,
            Some(&subject(Some(45))),
            50
        ));
        update_notification_preferences(
            &kv,
            &["local:/one"],
            &PreferencePatch::mute(Some(Mute::UntilResumed)),
            60,
        );
        assert!(!cue_allowed(
            &kv,
            SoundCue::TurnFinished,
            Some(&subject(Some(70))),
            80
        ));
    }

    fn card(updated_at: i64) -> LinkedWorkItemUpdateCard {
        LinkedWorkItemUpdateCard {
            kind: WorkItemKind::Pr,
            repo: "Acme/App".into(),
            number: 5,
            title: "Fix".into(),
            url: "https://github.com/Acme/App/pull/5".into(),
            state: "open".into(),
            since: 0,
            updated_at,
            status: LinkedWorkItemUpdateStatus::Ready,
            counts: LinkedWorkItemActivityCounts::default(),
            entries: Vec::new(),
            truncated: false,
        }
    }

    #[test]
    fn announces_linked_activity_once_per_session_and_update() {
        let mut cues = SoundCues::default();
        let first = cues
            .linked_activity_subject("s1", Some(&card(100)))
            .unwrap();
        assert_eq!(first.project_id, "repository:github.com/acme/app");
        assert_eq!(first.category, NotificationCategory::PullRequests);
        assert!(
            cues.linked_activity_subject("s1", Some(&card(100)))
                .is_none()
        );
        assert!(
            cues.linked_activity_subject("s2", Some(&card(100)))
                .is_some()
        );
        assert!(
            cues.linked_activity_subject("s1", Some(&card(200)))
                .is_some()
        );
        assert!(cues.linked_activity_subject("s1", None).is_none());
    }

    #[test]
    fn announces_each_available_version_once() {
        let mut cues = SoundCues::default();
        assert!(cues.update_available(Some("1.2.0")));
        assert!(!cues.update_available(Some("1.2.0")));
        assert!(!cues.update_available(None));
        assert!(cues.update_available(Some("1.2.0")));
        assert!(cues.update_available(Some("1.3.0")));
        cues.reset();
        assert!(cues.update_available(Some("1.3.0")));
    }
}
