//! Port of host/skills.ts, which mirrored src-tauri/src/skills.rs so a
//! project on a connected machine lists the skills installed there. That
//! Rust code now lives in `monocode_process::skills`, so the host calls it.

use std::path::Path;

pub use monocode_process::skills::DiscoveredSkill as HostSkill;

/// `listHostSkills`: skills visible for a project, `.agents/skills` first,
/// then native harness folders, then Claude plugins. The same name from an
/// earlier root wins, after disabled paths are skipped.
///
/// TODO(port): the TypeScript took the managed settings folder as a
/// parameter for tests. The shared lister always reads the system one.
pub fn list_host_skills(
    project: &Path,
    home: Option<&Path>,
    disabled_paths: Option<&[String]>,
) -> Vec<HostSkill> {
    monocode_process::skills::list_skills_from(project, home, disabled_paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Dirs(Vec<tempfile::TempDir>);

    impl Dirs {
        fn tmp(&mut self) -> PathBuf {
            let dir = tempfile::tempdir().unwrap();
            let path = std::fs::canonicalize(dir.path()).unwrap();
            self.0.push(dir);
            path
        }
    }

    fn write_skill(root: &Path, folder: &str, body: &str) {
        std::fs::create_dir_all(root.join(folder)).unwrap();
        std::fs::write(root.join(folder).join("SKILL.md"), body).unwrap();
    }

    fn skill(name: &str, description: &str) -> String {
        format!("---\nname: {name}\ndescription: {description}\n---\n")
    }

    fn list(project: &Path, home: &Path, disabled: Option<&[String]>) -> Vec<HostSkill> {
        list_host_skills(project, Some(home), disabled)
    }

    fn find<'a>(skills: &'a [HostSkill], name: &str) -> Option<&'a HostSkill> {
        skills.iter().find(|entry| entry.name == name)
    }

    /// skills.test.ts: "reads a folded description and falls back to the
    /// folder name", through the lister, since the parser is private there.
    #[test]
    fn reads_a_folded_description_and_falls_back_to_the_folder_name() {
        let mut dirs = Dirs(Vec::new());
        let project = dirs.tmp();
        let home = dirs.tmp();
        let root = project.join(".agents/skills");
        write_skill(
            &root,
            "fallback",
            "---\nname: review-pr\ndescription: >\n  Review pull requests.\n  Use when asked to review.\n---\n\n# hi\n",
        );
        write_skill(&root, "create-skill", "# no yaml\n");
        let skills = list(&project, &home, None);
        let review = find(&skills, "review-pr").unwrap();
        assert_eq!(
            review.description,
            "Review pull requests. Use when asked to review."
        );
        assert_eq!(find(&skills, "create-skill").unwrap().description, "");
    }

    /// skills.test.ts: "prefers .agents skills over provider folders and
    /// keeps both scopes".
    #[test]
    fn prefers_agents_skills_over_provider_folders_and_keeps_both_scopes() {
        let mut dirs = Dirs(Vec::new());
        let project = dirs.tmp();
        let home = dirs.tmp();
        write_skill(
            &project.join(".agents/skills"),
            "ship",
            &skill("ship", "MonoCode ship"),
        );
        write_skill(
            &project.join(".claude/skills"),
            "ship",
            &skill("ship", "Claude ship"),
        );
        write_skill(
            &home.join(".agents/skills"),
            "greet",
            &skill("greet", "Hello"),
        );
        write_skill(
            &home.join(".claude/skills"),
            "polish",
            &skill("polish", "Personal"),
        );
        write_skill(
            &project.join(".cursor/skills"),
            "cursor-only",
            &skill("cursor-only", "Cursor"),
        );
        std::fs::create_dir_all(home.join(".claude/skills/no-skill-md")).unwrap();

        let skills = list(&project, &home, None);
        let ship = find(&skills, "ship").unwrap();
        assert_eq!(
            (
                ship.description.as_str(),
                ship.source.as_str(),
                ship.scope.as_str()
            ),
            ("MonoCode ship", "agents", "project")
        );
        let greet = find(&skills, "greet").unwrap();
        assert_eq!(
            (greet.source.as_str(), greet.scope.as_str()),
            ("agents", "user")
        );
        let polish = find(&skills, "polish").unwrap();
        assert_eq!(
            (polish.source.as_str(), polish.scope.as_str()),
            ("claude", "user")
        );
        assert_eq!(
            polish.path,
            home.join(".claude/skills/polish/SKILL.md")
                .to_string_lossy()
        );
        assert_eq!(find(&skills, "cursor-only").unwrap().source, "cursor");
        assert!(find(&skills, "no-skill-md").is_none());
    }

    /// skills.test.ts: "lists enabled Claude plugin skills under their
    /// plugin's name".
    #[test]
    fn lists_enabled_claude_plugin_skills_under_their_plugins_name() {
        let mut dirs = Dirs(Vec::new());
        let project = dirs.tmp();
        let other = dirs.tmp();
        let home = dirs.tmp();
        let user_plugin = home.join(".claude/plugins/cache/community/workflow-kit/1.2.3");
        let project_plugin = home.join(".claude/plugins/cache/community/delivery/2.0.0");
        write_skill(
            &user_plugin.join("skills"),
            "quick-plan",
            &skill("quick-plan", "Plan from plugin"),
        );
        write_skill(
            &project_plugin.join("skills"),
            "ship-it",
            &skill("ship-it", "Deliver"),
        );
        write_skill(
            &home.join(".claude/skills"),
            "quick-plan",
            &skill("quick-plan", "Personal plan"),
        );
        std::fs::write(
            home.join(".claude/plugins/installed_plugins.json"),
            serde_json::json!({
                "version": 2,
                "plugins": {
                    "workflow-kit@community": [
                        { "scope": "user", "installPath": "~/.claude/plugins/cache/community/workflow-kit/1.2.3" }
                    ],
                    "delivery@community": [
                        { "scope": "project", "projectPath": project, "installPath": project_plugin }
                    ]
                }
            })
            .to_string(),
        )
        .unwrap();

        let nested = project.join("src");
        std::fs::create_dir(&nested).unwrap();
        let skills = list(&nested, &home, None);
        let planned = find(&skills, "workflow-kit:quick-plan").unwrap();
        assert_eq!(
            (
                planned.description.as_str(),
                planned.source.as_str(),
                planned.scope.as_str()
            ),
            ("Plan from plugin", "claude", "user")
        );
        assert!(find(&skills, "quick-plan").is_some());
        assert_eq!(find(&skills, "delivery:ship-it").unwrap().scope, "project");
        assert!(find(&list(&other, &home, None), "delivery:ship-it").is_none());

        std::fs::create_dir(project.join(".claude")).unwrap();
        std::fs::write(
            project.join(".claude/settings.local.json"),
            r#"{"enabledPlugins":{"workflow-kit@community":false}}"#,
        )
        .unwrap();
        assert!(find(&list(&nested, &home, None), "workflow-kit:quick-plan").is_none());
    }

    /// skills.test.ts: "falls back to a same-name personal skill when the
    /// project one is disabled".
    #[test]
    fn falls_back_to_a_same_name_personal_skill_when_the_project_one_is_disabled() {
        let mut dirs = Dirs(Vec::new());
        let project = dirs.tmp();
        let home = dirs.tmp();
        write_skill(
            &project.join(".agents/skills"),
            "review",
            &skill("review", "Project review"),
        );
        write_skill(
            &home.join(".agents/skills"),
            "review",
            &skill("review", "Personal review"),
        );
        let project_skill = project
            .join(".agents/skills/review/SKILL.md")
            .to_string_lossy()
            .into_owned();
        let personal_skill = home
            .join(".agents/skills/review/SKILL.md")
            .to_string_lossy()
            .into_owned();
        let review =
            |disabled: &[String]| find(&list(&project, &home, Some(disabled)), "review").cloned();
        assert_eq!(review(&[]).unwrap().description, "Project review");
        let fallback = review(std::slice::from_ref(&project_skill)).unwrap();
        assert_eq!(
            (fallback.description.as_str(), fallback.scope.as_str()),
            ("Personal review", "user")
        );
        assert_eq!(
            review(std::slice::from_ref(&personal_skill))
                .unwrap()
                .description,
            "Project review"
        );
        assert!(review(&[project_skill, personal_skill]).is_none());
    }
}
