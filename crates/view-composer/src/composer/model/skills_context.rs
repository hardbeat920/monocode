//! Port of the pure policies in src/features/sessions/ui/useComposerSkills.ts.
//! The hook's loading and subscriptions belong to the engine's skill
//! catalog; the composer keeps which rows it may show for its context.

use super::skills::Skill;

/// `ComposerSkillContextToken`: a context key plus a generation, so a
/// result for A that lands after A to B to A is not mistaken for current.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillContextToken {
    pub key: String,
    pub generation: u64,
}

/// `nextComposerSkillContextToken`.
pub fn next_skill_context_token(
    current: Option<&SkillContextToken>,
    key: &str,
) -> SkillContextToken {
    match current {
        Some(current) if current.key == key => current.clone(),
        _ => SkillContextToken {
            key: key.to_string(),
            generation: current.map_or(0, |current| current.generation + 1),
        },
    }
}

/// `pickerSkillLoadOptions`: opening the picker re-reads skill files from
/// disk, except for providers that report their own commands on a TTL.
pub fn picker_skill_refresh(has_native_commands: bool) -> bool {
    !has_native_commands
}

/// `visibleComposerSkills`: loaded rows only for their owning context.
pub fn visible_composer_skills(
    state_key: &str,
    state_skills: &[Skill],
    current_key: &str,
    cached: Option<&[Skill]>,
    fallback: &[Skill],
) -> Vec<Skill> {
    if state_key == current_key {
        state_skills.to_vec()
    } else {
        cached.unwrap_or(fallback).to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pi_skill() -> Skill {
        Skill::native("architect", "skill:architect", "Design first.", "pi")
    }

    fn cached_skill() -> Skill {
        Skill::native("cached", "skill:cached", "Cached current context.", "pi")
    }

    #[test]
    fn uses_loaded_rows_only_for_their_owning_context() {
        assert_eq!(
            visible_composer_skills("pi\0/a", &[pi_skill()], "pi\0/a", None, &[]),
            vec![pi_skill()]
        );
        assert_eq!(
            visible_composer_skills(
                "pi\0/a",
                &[pi_skill()],
                "pi\0/b",
                Some(&[cached_skill()]),
                &[]
            ),
            vec![cached_skill()]
        );
    }

    #[test]
    fn preserves_filesystem_refresh_while_native_providers_use_their_ttl() {
        assert!(!picker_skill_refresh(true));
        assert!(picker_skill_refresh(false));
    }

    #[test]
    fn does_not_reuse_a_context_token_after_a_to_b_to_a() {
        let first_a = next_skill_context_token(None, "pi\0/a");
        let same_a = next_skill_context_token(Some(&first_a), "pi\0/a");
        let b = next_skill_context_token(Some(&same_a), "pi\0/b");
        let second_a = next_skill_context_token(Some(&b), "pi\0/a");
        assert_eq!(same_a, first_a);
        assert_eq!(second_a.key, first_a.key);
        assert_ne!(second_a, first_a);
        assert!(second_a.generation > first_a.generation);
    }
}
