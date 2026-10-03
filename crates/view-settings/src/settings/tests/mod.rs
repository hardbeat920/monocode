//! GPUI behavior tests for the settings page: clicks and keys in a test
//! window, ported from SettingsView.test.ts, QuickComposerShortcutEditor.test.ts,
//! JiraSettings.test.ts, and ProjectBackgroundDialog.test.ts.

mod dialogs;
mod keybindings;
mod pages;
mod providers;
mod search;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use gpui::{
    App, Bounds, Entity, Modifiers, Pixels, Task, TestAppContext, VisualTestContext, px, size,
};
use monocode_core::Platform;
use monocode_core::harness::HarnessId;
use monocode_core::models::HarnessAvailability;
use monocode_core::settings::{KeybindingOverrides, SettingsSectionId};
use monocode_settings::Kv;
use monocode_ui::AppearanceSettings;

use super::host::*;
use super::page::{SectionBody, SettingsPage};

pub(super) fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        monocode_ui::init(AppearanceSettings::default(), cx);
    });
}

type Inspect = Box<dyn Fn(HarnessId, Option<&str>) -> Result<BinaryInspection, String>>;

/// One fake for every host trait, recording what the page asked for.
#[derive(Default)]
pub(super) struct TestHost {
    pub quick_composer: RefCell<Vec<(bool, Option<String>)>>,
    /// Errors the next quick composer registrations return, in order.
    pub quick_composer_failures: RefCell<VecDeque<String>>,
    pub overrides: RefCell<Vec<KeybindingOverrides>>,
    pub overrides_failure: RefCell<Option<String>>,
    pub installed: RefCell<Vec<HarnessId>>,
    pub inspect: RefCell<Option<Inspect>>,
    pub inspections: RefCell<Vec<(HarnessId, Option<String>)>>,
    pub revealed: RefCell<Vec<String>>,
    pub reveal_failure: RefCell<Option<String>>,
    pub jira: RefCell<JiraStatus>,
    pub jira_saves: RefCell<Vec<(String, String, String)>>,
    pub jira_failures: RefCell<VecDeque<String>>,
    pub jira_projects: RefCell<Vec<JiraProject>>,
    pub archived: RefCell<Vec<ArchivedProject>>,
    pub version: RefCell<Option<String>>,
}

impl GeneralHost for TestHost {
    fn app_version(&self, _: &mut App) -> Task<String> {
        Task::ready(
            self.version
                .borrow()
                .clone()
                .unwrap_or_else(|| "0.6.0".into()),
        )
    }
}

impl KeybindingsHost for TestHost {
    fn set_quick_composer_shortcut(
        &self,
        enabled: bool,
        shortcut: Option<&str>,
        _: &mut App,
    ) -> HostTask<()> {
        if let Some(error) = self.quick_composer_failures.borrow_mut().pop_front() {
            return Task::ready(Err(error));
        }
        self.quick_composer
            .borrow_mut()
            .push((enabled, shortcut.map(str::to_string)));
        Task::ready(Ok(()))
    }

    fn set_keybinding_overrides(
        &self,
        overrides: &KeybindingOverrides,
        _: &mut App,
    ) -> HostTask<()> {
        self.overrides.borrow_mut().push(overrides.clone());
        match self.overrides_failure.borrow().clone() {
            Some(error) => Task::ready(Err(error)),
            None => Task::ready(Ok(())),
        }
    }
}

impl AppearanceHost for TestHost {}

impl ProvidersHost for TestHost {
    fn availability(&self, _: &App) -> HarnessAvailability {
        HarnessAvailability {
            installed: self.installed.borrow().iter().copied().collect(),
            probed: true,
        }
    }

    fn inspect_binary(
        &self,
        provider: HarnessId,
        path: Option<&str>,
        _: &mut App,
    ) -> HostTask<BinaryInspection> {
        self.inspections
            .borrow_mut()
            .push((provider, path.map(str::to_string)));
        let result = match self.inspect.borrow().as_ref() {
            Some(inspect) => inspect(provider, path),
            None => Err(format!("{} CLI not found", provider.title())),
        };
        Task::ready(result)
    }

    fn reveal_path(&self, path: &str, _: &mut App) -> HostTask<()> {
        self.revealed.borrow_mut().push(path.to_string());
        match self.reveal_failure.borrow().clone() {
            Some(error) => Task::ready(Err(error)),
            None => Task::ready(Ok(())),
        }
    }
}

impl InboxHost for TestHost {
    fn jira_status(&self, _: &mut App) -> HostTask<JiraStatus> {
        Task::ready(Ok(self.jira.borrow().clone()))
    }

    fn save_jira_config(
        &self,
        site: &str,
        email: &str,
        token: &str,
        _: &mut App,
    ) -> HostTask<JiraStatus> {
        if let Some(error) = self.jira_failures.borrow_mut().pop_front() {
            return Task::ready(Err(error));
        }
        self.jira_saves
            .borrow_mut()
            .push((site.into(), email.into(), token.into()));
        let status = JiraStatus {
            connected: !token.is_empty(),
            site: site.into(),
            email: email.into(),
        };
        *self.jira.borrow_mut() = status.clone();
        Task::ready(Ok(status))
    }

    fn list_jira_projects(&self, _: &mut App) -> HostTask<Vec<JiraProject>> {
        Task::ready(Ok(self.jira_projects.borrow().clone()))
    }
}

impl ArchiveHost for TestHost {
    fn archived_projects(&self, _: &App) -> Vec<ArchivedProject> {
        self.archived.borrow().clone()
    }
}

pub(super) fn hosts(host: &Rc<TestHost>) -> SettingsHosts {
    SettingsHosts {
        general: host.clone(),
        keybindings: host.clone(),
        appearance: host.clone(),
        providers: host.clone(),
        inbox: host.clone(),
        archive: host.clone(),
        ..Default::default()
    }
}

type Callback<T> = Option<Rc<dyn Fn(T, &mut gpui::Window, &mut App)>>;

/// Records every value a callback receives.
pub(super) struct Calls<T>(Rc<RefCell<Vec<T>>>);

impl<T: Clone + 'static> Calls<T> {
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(Vec::new())))
    }

    pub fn callback(&self) -> Callback<T> {
        let calls = self.0.clone();
        Some(Rc::new(move |value, _, _| calls.borrow_mut().push(value)))
    }

    pub fn all(&self) -> Vec<T> {
        self.0.borrow().clone()
    }
}

/// What a test page is built from.
pub(super) struct Setup {
    pub kv: Kv,
    pub platform: Platform,
    pub host: Rc<TestHost>,
    pub props: SettingsProps,
    pub callbacks: SettingsCallbacks,
}

impl Setup {
    pub fn new(platform: Platform) -> Self {
        Self {
            kv: Kv::in_memory(),
            platform,
            host: Rc::new(TestHost::default()),
            props: SettingsProps {
                cwd: "/repo".into(),
                ..Default::default()
            },
            callbacks: SettingsCallbacks::default(),
        }
    }
}

/// Opens a tall window with the settings page on `section`.
pub(super) fn mount(
    cx: &mut TestAppContext,
    section: SettingsSectionId,
    setup: &Setup,
) -> (Entity<SettingsPage>, &'static mut VisualTestContext) {
    mount_sized(cx, section, setup, 1200., 4000.)
}

/// Opens the settings page in a window of the given size.
pub(super) fn mount_sized(
    cx: &mut TestAppContext,
    section: SettingsSectionId,
    setup: &Setup,
    width: f32,
    height: f32,
) -> (Entity<SettingsPage>, &'static mut VisualTestContext) {
    init(cx);
    let (kv, platform, hosts, props, callbacks) = (
        setup.kv.clone(),
        setup.platform,
        hosts(&setup.host),
        setup.props.clone(),
        setup.callbacks.clone(),
    );
    let window = cx.open_window(size(px(width), px(height)), move |window, cx| {
        SettingsPage::new(kv, platform, hosts, section, props, callbacks, window, cx)
    });
    let page = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    draw(cx);
    (page, cx)
}

pub(super) fn draw(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.update(|window, cx| {
            window.draw(cx).clear();
        });
        cx.run_until_parked();
    }
}

pub(super) fn leak(selector: String) -> &'static str {
    Box::leak(selector.into_boxed_str())
}

pub(super) fn bounds(cx: &mut VisualTestContext, selector: &str) -> Bounds<Pixels> {
    let selector = leak(selector.to_string());
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("no element {selector}"))
}

pub(super) fn exists(cx: &mut VisualTestContext, selector: &str) -> bool {
    let selector = leak(selector.to_string());
    cx.debug_bounds(selector).is_some()
}

pub(super) fn click(cx: &mut VisualTestContext, selector: &str) {
    let at = bounds(cx, selector).center();
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
}

pub(super) fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    draw(cx);
}

pub(super) fn modifiers(cx: &mut VisualTestContext, modifiers: Modifiers) {
    cx.simulate_modifiers_change(modifiers);
    draw(cx);
}

/// The open section's view, when it is the kind asked for.
pub(super) fn body(page: &Entity<SettingsPage>, cx: &mut VisualTestContext) -> SectionBody {
    page.read_with(cx, |page, _| page.body().clone())
}
