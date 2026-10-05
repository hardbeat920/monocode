//! Port of src/features/workspace/model/tabGroups.test.ts.

use super::*;
use crate::paths::normalize_project_path;

/// The title-bar `Tab` fields the grouping helpers read.
#[derive(Debug, Clone, PartialEq)]
struct TitleTab {
    id: String,
    project: String,
    group_id: Option<String>,
}

impl GroupedTab for TitleTab {
    fn id(&self) -> &str {
        &self.id
    }

    fn group_id(&self) -> Option<&str> {
        self.group_id.as_deref()
    }

    fn set_group_id(&mut self, group_id: Option<String>) {
        self.group_id = group_id;
    }
}

impl ProjectTab for TitleTab {
    fn project(&self) -> &str {
        &self.project
    }
}

fn tab(id: &str, project: &str, group_id: Option<&str>) -> TitleTab {
    TitleTab {
        id: id.into(),
        project: project.into(),
        group_id: group_id.map(str::to_string),
    }
}

fn ids(tabs: &[TitleTab]) -> Vec<&str> {
    tabs.iter().map(|tab| tab.id.as_str()).collect()
}

fn groups(tabs: &[TitleTab]) -> Vec<(&str, Option<&str>)> {
    tabs.iter()
        .map(|tab| (tab.id.as_str(), tab.group_id.as_deref()))
        .collect()
}

fn order(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| id.to_string()).collect()
}

/// Projects live on the title tab, so callers pass this lookup explicitly.
fn project_of(tabs: &[TitleTab]) -> impl Fn(&str) -> Option<String> + use<> {
    let tabs = tabs.to_vec();
    move |id| {
        tabs.iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.project.clone())
    }
}

// segmentTabs

#[test]
fn leaves_ungrouped_tabs_as_singles_even_when_they_share_a_project() {
    let tabs = [tab("a", "foo", None), tab("b", "foo", None)];
    let segments = segment_tabs(&tabs);
    assert_eq!(segments.len(), 2);
    assert!(
        segments
            .iter()
            .all(|segment| matches!(segment, TabGroupSegment::Single { .. }))
    );
}

#[test]
fn groups_contiguous_tabs_that_share_a_group_id_including_a_single_tab() {
    let tabs = [
        tab("a", "foo", Some("g1")),
        tab("b", "foo", Some("g1")),
        tab("c", "bar", None),
    ];
    let segments = segment_tabs(&tabs);
    let TabGroupSegment::Group {
        key,
        project,
        tabs: run,
        start_index,
        color,
    } = &segments[0]
    else {
        panic!("expected a group");
    };
    assert_eq!(key, "g1");
    assert_eq!(project, "foo");
    assert_eq!(ids(run), vec!["a", "b"]);
    assert_eq!(*start_index, 0);
    assert_eq!(*color, tab_group_color("g1"));
    assert!(matches!(segments[1], TabGroupSegment::Single { tab, index: 2 } if tab.id == "c"));
}

// sharedGroupProject

#[test]
fn shared_group_project_returns_the_project_when_every_tab_matches() {
    assert_eq!(
        shared_group_project(&[tab("a", "foo", None), tab("b", "foo", None)]).as_deref(),
        Some("foo")
    );
}

#[test]
fn shared_group_project_returns_none_for_mixed_or_missing_projects() {
    assert_eq!(
        shared_group_project(&[tab("a", "foo", None), tab("b", "bar", None)]),
        None
    );
    assert_eq!(shared_group_project(&[tab("a", "~", None)]), None);
}

// applyGroupedReorder

fn grouped_fixture() -> Vec<TitleTab> {
    vec![
        tab("a", "foo", Some("g")),
        tab("b", "foo", Some("g")),
        tab("c", "bar", None),
    ]
}

#[test]
fn joins_an_ungrouped_tab_dropped_between_two_members_of_the_same_group() {
    let next =
        apply_grouped_reorder(&grouped_fixture(), &order(&["a", "c", "b"]), "c", None).unwrap();
    assert_eq!(
        groups(&next),
        vec![("a", Some("g")), ("c", Some("g")), ("b", Some("g"))]
    );
}

#[test]
fn ungroups_a_tab_dragged_out_of_its_group() {
    let next =
        apply_grouped_reorder(&grouped_fixture(), &order(&["a", "c", "b"]), "b", None).unwrap();
    assert_eq!(
        groups(&next),
        vec![("a", Some("g")), ("c", None), ("b", None)]
    );
}

#[test]
fn keeps_membership_when_sliding_a_grouped_tab_along_its_own_group() {
    let next =
        apply_grouped_reorder(&grouped_fixture(), &order(&["b", "a", "c"]), "b", None).unwrap();
    assert_eq!(
        groups(&next),
        vec![("b", Some("g")), ("a", Some("g")), ("c", None)]
    );
}

#[test]
fn does_not_auto_join_when_placed_beside_a_group_edge() {
    let next =
        apply_grouped_reorder(&grouped_fixture(), &order(&["a", "b", "c"]), "c", None).unwrap();
    assert_eq!(
        groups(&next),
        vec![("a", Some("g")), ("b", Some("g")), ("c", None)]
    );
}

#[test]
fn rejects_an_order_that_is_not_a_permutation() {
    assert_eq!(
        apply_grouped_reorder(&grouped_fixture(), &order(&["a", "b"]), "a", None),
        None
    );
    assert_eq!(
        apply_grouped_reorder(&grouped_fixture(), &order(&["a", "b", "x"]), "a", None),
        None
    );
}

// joinTabOnto

#[test]
fn creates_a_group_from_two_ungrouped_tabs_and_places_the_dragged_tab_after_the_target() {
    let tabs = [
        tab("a", "foo", None),
        tab("b", "bar", None),
        tab("c", "baz", None),
    ];
    let create = || "g-new".to_string();
    let result = join_tab_onto(&tabs, "c", "a", Some(&create), None).unwrap();
    assert!(result.created);
    assert_eq!(result.group_id, "g-new");
    assert_eq!(
        groups(&result.tabs),
        vec![("a", Some("g-new")), ("c", Some("g-new")), ("b", None)]
    );
}

#[test]
fn joins_a_tab_onto_an_existing_group() {
    let result = join_tab_onto(&grouped_fixture(), "c", "a", None, None).unwrap();
    assert!(!result.created);
    assert_eq!(
        groups(&result.tabs),
        vec![("a", Some("g")), ("b", Some("g")), ("c", Some("g"))]
    );
}

// group membership helpers

#[test]
fn adds_a_tab_to_the_end_of_an_existing_group() {
    let tabs = [
        tab("a", "foo", Some("g")),
        tab("b", "bar", None),
        tab("c", "foo", Some("g")),
    ];
    assert_eq!(
        ids(&add_tab_to_group(&tabs, "b", "g", None)),
        vec!["a", "c", "b"]
    );
}

#[test]
fn creates_a_one_tab_group_in_place() {
    let tabs = [tab("a", "foo", None), tab("b", "bar", None)];
    assert_eq!(
        groups(&add_tabs_to_new_group(&tabs, &order(&["b"]), "g")),
        vec![("a", None), ("b", Some("g"))]
    );
}

#[test]
fn clears_a_group_and_a_single_tabs_membership() {
    let tabs = [
        tab("a", "foo", Some("g")),
        tab("b", "foo", Some("g")),
        tab("c", "bar", Some("g2")),
    ];
    assert_eq!(
        ungroup_tabs(&tabs, "g")
            .iter()
            .map(|entry| entry.group_id.as_deref())
            .collect::<Vec<_>>(),
        vec![None, None, Some("g2")]
    );
    assert_eq!(remove_tab_from_group(&tabs, "a")[0].group_id, None);
}

#[test]
fn inserts_a_new_tab_beside_the_active_tab_and_inherits_its_group() {
    let tabs = [tab("a", "foo", Some("g")), tab("b", "bar", None)];
    let next = insert_tab_beside_active(&tabs, tab("n", "foo", None), Some("a"), None);
    assert_eq!(
        groups(&next),
        vec![("a", Some("g")), ("n", Some("g")), ("b", None)]
    );
}

#[test]
fn inserts_an_ungrouped_tab_after_the_active_ungrouped_tab() {
    let tabs = [tab("a", "foo", None), tab("b", "bar", None)];
    let next = insert_tab_beside_active(&tabs, tab("n", "foo", None), Some("a"), None);
    assert_eq!(groups(&next), vec![("a", None), ("n", None), ("b", None)]);
}

#[test]
fn inserts_into_a_group_after_its_last_member() {
    let next = insert_tab_in_group(&grouped_fixture(), tab("n", "foo", None), "g");
    assert_eq!(ids(&next), vec!["a", "b", "n", "c"]);
    assert_eq!(next[2].group_id.as_deref(), Some("g"));
}

// tab group logos

#[test]
fn resolves_logo_paths_by_project_key() {
    let mut logos = JsRecord::new();
    logos.insert("foo", "/tmp/foo.png".to_string());
    assert_eq!(
        resolve_tab_group_logo("foo", Some(&logos)).as_deref(),
        Some("/tmp/foo.png")
    );
    assert_eq!(resolve_tab_group_logo("bar", Some(&logos)), None);
}

// reorderTabSegments

#[test]
fn moves_a_whole_group_before_another_segment() {
    let tabs = [
        tab("a", "foo", Some("g1")),
        tab("b", "foo", Some("g1")),
        tab("c", "bar", None),
        tab("d", "baz", None),
    ];
    assert_eq!(
        reorder_tab_segments(&tabs, 0, 1),
        Some(order(&["c", "a", "b", "d"]))
    );
}

#[test]
fn moves_a_group_after_ungrouped_tabs() {
    let tabs = [
        tab("a", "foo", Some("g1")),
        tab("b", "foo", Some("g1")),
        tab("c", "bar", None),
    ];
    assert_eq!(
        reorder_tab_segments(&tabs, 0, 1),
        Some(order(&["c", "a", "b"]))
    );
    assert_eq!(
        reorder_tab_segments(&tabs, 0, 0),
        Some(order(&["a", "b", "c"]))
    );
    assert_eq!(reorder_tab_segments(&tabs, 0, 2), None);
}

#[test]
fn swaps_two_groups() {
    let tabs = [
        tab("a", "foo", Some("g1")),
        tab("b", "foo", Some("g1")),
        tab("c", "bar", Some("g2")),
        tab("d", "bar", Some("g2")),
    ];
    assert_eq!(
        reorder_tab_segments(&tabs, 0, 1),
        Some(order(&["c", "d", "a", "b"]))
    );
}

// project-scoped grouping

fn scoped_fixture() -> Vec<TitleTab> {
    vec![
        tab("a", "foo", Some("g")),
        tab("b", "foo", Some("g")),
        tab("c", "bar", None),
        tab("d", "foo", None),
    ]
}

#[test]
fn refuses_a_tab_from_another_project_on_a_tab_and_on_a_group() {
    let tabs = scoped_fixture();
    let lookup = project_of(&tabs);
    assert!(!can_join_tab_onto(&tabs, "c", "a", Some(&lookup)));
    assert!(!can_join_tab_onto(&tabs, "c", "d", Some(&lookup)));
    assert!(!can_join_tab_group(&tabs, "c", "g", Some(&lookup)));
}

#[test]
fn allows_tabs_that_share_a_project() {
    let tabs = scoped_fixture();
    let lookup = project_of(&tabs);
    assert!(can_join_tab_onto(&tabs, "d", "a", Some(&lookup)));
    assert!(can_join_tab_group(&tabs, "d", "g", Some(&lookup)));
}

#[test]
fn leaves_the_tabs_untouched_when_a_cross_project_join_is_attempted() {
    let tabs = scoped_fixture();
    let lookup = project_of(&tabs);
    let create = || "g-new".to_string();
    assert_eq!(
        join_tab_onto(&tabs, "c", "a", Some(&create), Some(&lookup)),
        None
    );
    assert_eq!(add_tab_to_group(&tabs, "c", "g", Some(&lookup)), tabs);
}

#[test]
fn slides_a_foreign_tab_past_a_group_instead_of_joining_or_splitting_it() {
    // Dragged leftwards into the middle of `g`, so it lands before the group.
    let tabs = scoped_fixture();
    let lookup = project_of(&tabs);
    let next =
        apply_grouped_reorder(&tabs, &order(&["a", "c", "b", "d"]), "c", Some(&lookup)).unwrap();
    assert_eq!(
        groups(&next),
        vec![("c", None), ("a", Some("g")), ("b", Some("g")), ("d", None)]
    );
}

#[test]
fn slides_a_foreign_tab_out_on_the_far_side_when_dragged_rightwards() {
    let rightward = [
        tab("c", "bar", None),
        tab("a", "foo", Some("g")),
        tab("b", "foo", Some("g")),
    ];
    let lookup = project_of(&rightward);
    let next =
        apply_grouped_reorder(&rightward, &order(&["a", "c", "b"]), "c", Some(&lookup)).unwrap();
    assert_eq!(
        groups(&next),
        vec![("a", Some("g")), ("b", Some("g")), ("c", None)]
    );
}

#[test]
fn still_joins_a_group_when_the_dropped_tab_shares_its_project() {
    let tabs = scoped_fixture();
    let lookup = project_of(&tabs);
    let next =
        apply_grouped_reorder(&tabs, &order(&["a", "d", "b", "c"]), "d", Some(&lookup)).unwrap();
    assert_eq!(
        groups(&next),
        vec![
            ("a", Some("g")),
            ("d", Some("g")),
            ("b", Some("g")),
            ("c", None)
        ]
    );
}

#[test]
fn does_not_inherit_the_active_tabs_group_across_projects() {
    let tabs = scoped_fixture();
    let lookup = project_of(&tabs);
    let with_new = |id: &str| {
        if id == "n" {
            Some("bar".to_string())
        } else {
            lookup(id)
        }
    };
    let next = insert_tab_beside_active(&tabs, tab("n", "bar", None), Some("a"), Some(&with_new));
    assert_eq!(
        groups(&next),
        vec![
            ("a", Some("g")),
            ("n", None),
            ("b", Some("g")),
            ("c", None),
            ("d", None)
        ]
    );
}

// project appearance keys

const FINANCE: &str = "/Users/me/cortex-finance/agentbase";
const CORTEX: &str = "/Users/me/cortex/agentbase";

fn store(seed: &[(&str, &str)], known: &[&str]) -> MemoryStore {
    MemoryStore {
        items: seed
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect(),
        known_paths: known
            .iter()
            .map(|path| normalize_project_path(path))
            .collect(),
    }
}

#[test]
fn keeps_same_named_projects_in_different_folders_apart() {
    let mut store = store(&[(KEY_VERSION_KEY, "2")], &[]);
    let mut appearance = TabGroupAppearance::new();
    // Both checkouts share a folder name, the collision this guards against.
    assert_eq!(project_name(FINANCE), project_name(CORTEX));
    assert_ne!(project_key(FINANCE), project_key(CORTEX));

    appearance.save_tab_group_label(&mut store, &project_key(FINANCE), "Finance");
    appearance.save_tab_group_color(&mut store, &project_key(FINANCE), Some(3));

    let labels = appearance.load_tab_group_labels(&mut store);
    assert_eq!(
        resolve_tab_group_label(&project_key(FINANCE), Some(&labels), "agentbase"),
        "Finance"
    );
    assert_eq!(
        resolve_tab_group_label(&project_key(CORTEX), Some(&labels), "agentbase"),
        "agentbase"
    );
    assert_eq!(
        appearance
            .load_tab_group_colors(&mut store)
            .get(&project_key(CORTEX)),
        None
    );
    assert_eq!(
        appearance
            .load_tab_group_colors(&mut store)
            .get(&project_key(FINANCE)),
        Some(&3)
    );
    assert_eq!(
        appearance.take_events(),
        vec![
            AppearanceEvent::LabelsChanged,
            AppearanceEvent::ProjectPathsChanged,
            AppearanceEvent::ProjectPathsChanged,
        ]
    );
}

// migrateProjectAppearanceKeys

#[test]
fn moves_folder_name_entries_onto_every_project_that_carries_the_name() {
    let mut store = store(
        &[
            (LABEL_KEY, r#"{"agentbase":"Agentbase"}"#),
            (COLOR_KEY, r#"{"agentbase":"4"}"#),
        ],
        &[FINANCE, CORTEX],
    );
    let mut appearance = TabGroupAppearance::new();

    let labels = appearance.load_tab_group_labels(&mut store);
    assert_eq!(
        labels.get(&project_key(FINANCE)).map(String::as_str),
        Some("Agentbase")
    );
    assert_eq!(
        labels.get(&project_key(CORTEX)).map(String::as_str),
        Some("Agentbase")
    );
    assert_eq!(labels.get("agentbase"), None);
    assert_eq!(
        appearance
            .load_tab_group_colors(&mut store)
            .get(&project_key(CORTEX)),
        Some(&4)
    );
    assert_eq!(
        store.items.get(KEY_VERSION_KEY).map(String::as_str),
        Some("2")
    );

    // Renaming one afterwards leaves the other alone.
    appearance.save_tab_group_label(&mut store, &project_key(CORTEX), "Cortex");
    let after = appearance.load_tab_group_labels(&mut store);
    assert_eq!(
        after.get(&project_key(CORTEX)).map(String::as_str),
        Some("Cortex")
    );
    assert_eq!(
        after.get(&project_key(FINANCE)).map(String::as_str),
        Some("Agentbase")
    );
}

#[test]
fn leaves_entries_for_projects_it_no_longer_knows_about() {
    let mut store = store(&[(LABEL_KEY, r#"{"gone":"Gone"}"#)], &[]);
    let mut appearance = TabGroupAppearance::new();
    assert_eq!(
        appearance
            .load_tab_group_labels(&mut store)
            .get("gone")
            .map(String::as_str),
        Some("Gone")
    );
    // Unfinished: a later launch must still get the chance to claim it.
    assert_eq!(store.items.get(KEY_VERSION_KEY), None);
}

#[test]
fn claims_an_entry_once_its_project_is_remembered_again() {
    // Evicted from the 20-slot recents cap, so the first pass cannot match it.
    let mut first = store(&[(LABEL_KEY, r#"{"agentbase":"Finance"}"#)], &[]);
    assert_eq!(
        TabGroupAppearance::new()
            .load_tab_group_labels(&mut first)
            .get(&project_key(FINANCE)),
        None
    );

    // Next launch, with the project reopened.
    let mut second = store(&[(LABEL_KEY, r#"{"agentbase":"Finance"}"#)], &[FINANCE]);
    assert_eq!(
        TabGroupAppearance::new()
            .load_tab_group_labels(&mut second)
            .get(&project_key(FINANCE))
            .map(String::as_str),
        Some("Finance")
    );
}

#[test]
fn keeps_windows_checkouts_that_differ_only_in_case_together() {
    let mut store = store(
        &[(LABEL_KEY, r#"{"Agentbase":"Finance"}"#)],
        &["C:\\Users\\me\\cortex\\Agentbase"],
    );
    let labels = TabGroupAppearance::new().load_tab_group_labels(&mut store);
    assert_eq!(
        labels
            .get(&project_key("C:/Users/me/cortex/agentbase"))
            .map(String::as_str),
        Some("Finance")
    );
}

#[test]
fn a_newer_path_entry_wins_over_an_old_folder_name_entry() {
    // The folder-name entry is older, so it comes first in the record.
    let raw = format!(r#"{{"agentbase":"Old","{}":"New"}}"#, project_key(FINANCE));
    let mut store = store(&[(LABEL_KEY, raw.as_str())], &[FINANCE]);
    let labels = TabGroupAppearance::new().load_tab_group_labels(&mut store);
    assert_eq!(
        labels.get(&project_key(FINANCE)).map(String::as_str),
        Some("New")
    );
}

// resolveTabGroupColor

#[test]
fn falls_back_to_the_folder_name_hash_so_existing_rails_keep_their_color() {
    let empty_index = JsRecord::new();
    let empty_custom = JsRecord::new();
    assert_eq!(
        resolve_tab_group_color(
            &project_key(FINANCE),
            Some(&empty_index),
            Some(&empty_custom),
            Some("agentbase")
        ),
        resolve_tab_group_color(
            "agentbase",
            Some(&empty_index),
            Some(&empty_custom),
            Some("agentbase")
        )
    );
}

// Behavior the TypeScript tests reach only through callers.

#[test]
fn custom_colors_replace_palette_indexes() {
    let mut store = store(&[(KEY_VERSION_KEY, "2")], &[]);
    let mut appearance = TabGroupAppearance::new();
    appearance.save_tab_group_color(&mut store, "/p", Some(2));
    appearance.save_tab_group_custom_color(&mut store, "/p", Some("#A1B2C3"));
    let colors = appearance.load_tab_group_colors(&mut store);
    let custom = appearance.load_tab_group_custom_colors(&mut store);
    assert_eq!(colors.get("/p"), None);
    assert_eq!(custom.get("/p").map(String::as_str), Some("#a1b2c3"));
    assert_eq!(
        resolve_tab_group_color("/p", Some(&colors), Some(&custom), None),
        "#a1b2c3"
    );
    assert_eq!(
        resolve_tab_group_color_index("/p", Some(&colors), Some(&custom)),
        None
    );

    appearance.save_tab_group_color(&mut store, "/p", Some(5));
    let colors = appearance.load_tab_group_colors(&mut store);
    let custom = appearance.load_tab_group_custom_colors(&mut store);
    assert_eq!(
        resolve_tab_group_color_index("/p", Some(&colors), Some(&custom)),
        Some(5)
    );
    assert_eq!(
        resolve_tab_group_color("/p", Some(&colors), Some(&custom), None),
        TAB_GROUP_COLORS[5]
    );
    assert_eq!(
        store.items.get(COLOR_KEY).map(String::as_str),
        Some(r#"{"/p":"5"}"#)
    );
}

#[test]
fn rebases_and_clears_project_overrides() {
    let mut store = store(&[(KEY_VERSION_KEY, "2")], &[]);
    let mut appearance = TabGroupAppearance::new();
    appearance.save_tab_group_label(&mut store, "/old", "Old");
    appearance.save_tab_group_logo(&mut store, "/old", Some("/tmp/logo.png"));
    appearance.save_tab_group_mascot(&mut store, "/old", Some("fox"));
    appearance.take_events();

    appearance.rebase_project_tab_group_settings(&mut store, "/old/", "/new");
    assert_eq!(
        appearance
            .load_tab_group_labels(&mut store)
            .get("/new")
            .map(String::as_str),
        Some("Old")
    );
    assert_eq!(
        resolve_tab_group_logo("/new", Some(&appearance.load_tab_group_logos(&mut store)))
            .as_deref(),
        Some("/tmp/logo.png")
    );
    assert_eq!(
        resolve_tab_group_mascot("/new", Some(&appearance.load_tab_group_mascots(&mut store)))
            .as_deref(),
        Some("fox")
    );
    assert_eq!(appearance.tab_group_logo_display_revision(), 1);
    assert_eq!(
        appearance.take_events(),
        vec![
            AppearanceEvent::LabelsChanged,
            AppearanceEvent::LogosChanged
        ]
    );

    appearance.clear_tab_group_settings(&mut store, "/new");
    assert!(appearance.load_tab_group_labels(&mut store).is_empty());
    assert!(appearance.load_tab_group_mascots(&mut store).is_empty());
    assert_eq!(
        appearance.take_events(),
        vec![AppearanceEvent::LabelsChanged]
    );
}

#[test]
fn reads_records_like_object_entries() {
    let mut store = store(
        &[
            (KEY_VERSION_KEY, "2"),
            (LABEL_KEY, r#"{"b":"1","10":"x","2":"y","a":5,"c":"3"}"#),
            (MASCOT_KEY, r#"["fox", 1, "owl"]"#),
            (LOGO_KEY, "not json"),
        ],
        &[],
    );
    let mut appearance = TabGroupAppearance::new();
    let labels = appearance.load_tab_group_labels(&mut store);
    assert_eq!(
        labels
            .iter()
            .map(|(key, value)| (key, value.as_str()))
            .collect::<Vec<_>>(),
        vec![("2", "y"), ("10", "x"), ("b", "1"), ("c", "3")]
    );
    let mascots = appearance.load_tab_group_mascots(&mut store);
    assert_eq!(
        mascots
            .iter()
            .map(|(key, value)| (key, value.as_str()))
            .collect::<Vec<_>>(),
        vec![("0", "fox"), ("2", "owl")]
    );
    assert!(appearance.load_tab_group_logos(&mut store).is_empty());
}

#[test]
fn round_trips_collapsed_groups() {
    let mut store = store(&[(COLLAPSED_KEY, r#"["g1", 2, "g2", "g1"]"#)], &[]);
    assert_eq!(load_collapsed_tab_groups(&store), order(&["g1", "g2"]));
    save_collapsed_tab_groups(&mut store, &order(&["g3"]));
    assert_eq!(
        store.items.get(COLLAPSED_KEY).map(String::as_str),
        Some(r#"["g3"]"#)
    );
    assert!(load_collapsed_tab_groups(&MemoryStore::default()).is_empty());
}

#[test]
fn hashes_group_keys_onto_the_colored_palette_entries() {
    let color = tab_group_color("agentbase");
    assert_ne!(color, TAB_GROUP_COLORS[0]);
    assert_eq!(tab_group_color("agentbase"), color);
    // "a" is UTF-16 code 97: 97 % 8 + 1 = 2.
    assert_eq!(tab_group_color("a"), TAB_GROUP_COLORS[2]);
}

#[test]
fn workspace_tabs_carry_their_group() {
    let tabs = vec![
        WorkspaceTab {
            group_id: Some("g".into()),
            ..crate::layout::new_tab("s1")
        },
        crate::layout::new_tab("s2"),
    ];
    let next = ungroup_tabs(&tabs, "g");
    assert!(next.iter().all(|tab| tab.group_id.is_none()));
}
