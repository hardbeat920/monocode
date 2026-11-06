//! The Device tab's tool rail for iOS Simulators: a column of tools between
//! the conversation and the phone. Each opens a flyout over the conversation,
//! so the phone stays live while you change it, or pins under the phone.
//! Below the phone, a strip lists everything that differs from how the
//! simulator booted, so a forgotten setting cannot quietly skew later work.
use super::{
    DeviceEvent, DevicePanel,
    sim::{self, Grant, LogLine, Permission, Setting, Settings},
};
use crate::{
    input::Input,
    popover::{self, Align, Anchor, AnchorSlot, Side},
    theme::*,
    tooltip::Tip,
};
use gpui::{
    AnyElement, Context, Entity, FontWeight, KeyDownEvent, ObjectFit, Role, SharedString, Stateful,
    StyledImage, Task, Window, div, img, prelude::*, px, rgb,
};
use monocode_core::device::{Platform, hub};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::PathBuf,
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum Tool {
    Display,
    Accessibility,
    App,
    Location,
    Permissions,
    Push,
    Logs,
    Presets,
    Shots,
    Record,
}

/// The rail top to bottom; `None` is a divider between groups.
const RAIL: [Option<Tool>; 12] = [
    Some(Tool::Display),
    Some(Tool::Accessibility),
    None,
    Some(Tool::App),
    Some(Tool::Location),
    Some(Tool::Permissions),
    Some(Tool::Push),
    Some(Tool::Logs),
    None,
    Some(Tool::Presets),
    Some(Tool::Shots),
    Some(Tool::Record),
];

impl Tool {
    fn label(self) -> &'static str {
        match self {
            Self::Display => "Display",
            Self::Accessibility => "Accessibility",
            Self::App => "App",
            Self::Location => "Location",
            Self::Permissions => "Permissions",
            Self::Push => "Push notification",
            Self::Logs => "Logs",
            Self::Presets => "Presets",
            Self::Shots => "Screenshot matrix",
            Self::Record => "Record the screen",
        }
    }
    fn icon(self) -> &'static str {
        match self {
            Self::Display => "device-display",
            Self::Accessibility => "device-accessibility",
            Self::App => "app-window",
            Self::Location => "map-pin",
            Self::Permissions => "shield-check",
            Self::Push => "bell",
            Self::Logs => "terminal",
            Self::Presets => "star",
            Self::Shots => "screenshots",
            Self::Record => "record",
        }
    }
    /// ⌥ and this digit opens the tool while the panel has focus.
    fn key(self) -> Option<&'static str> {
        Some(match self {
            Self::Display => "1",
            Self::Accessibility => "2",
            Self::App => "3",
            Self::Location => "4",
            Self::Permissions => "5",
            Self::Push => "6",
            Self::Logs => "7",
            Self::Presets => "8",
            _ => return None,
        })
    }
    fn from_key(key: &str) -> Option<Self> {
        RAIL.into_iter()
            .flatten()
            .find(|tool| tool.key() == Some(key))
    }
    /// Tools worth keeping open while you use the phone.
    fn pinnable(self) -> bool {
        !matches!(self, Self::Presets | Self::Shots | Self::Record)
    }
    fn index(self) -> usize {
        self as usize
    }
}

/// Something set that a fresh simulator does not have.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Override {
    Setting(Setting),
    Location,
    Permission(Permission),
}

const PLACES: [(&str, f64, f64); 6] = [
    ("Cupertino", 37.3349, -122.009),
    ("New York", 40.7128, -74.006),
    ("London", 51.5074, -0.1278),
    ("Sofia", 42.6977, 23.3219),
    ("Tokyo", 35.6762, 139.6503),
    ("Sydney", -33.8688, 151.2093),
];

const PUSHES: [(&str, &str); 3] = [
    (
        "Alert",
        r#"{"aps":{"alert":{"title":"Hello","body":"Sent from MonoCode"},"sound":"default"}}"#,
    ),
    ("Badge 3", r#"{"aps":{"badge":3}}"#),
    ("Silent", r#"{"aps":{"content-available":1}}"#),
];

#[derive(Clone, Copy)]
enum Change {
    Setting(Setting, &'static str),
    Place(usize),
    ResetPermissions,
}

const PRESETS: [(&str, &str, &[Change]); 4] = [
    (
        "Accessibility stress",
        "AX5 text, Increase Contrast, Reduce Motion, Button Shapes",
        &[
            Change::Setting(Setting::TextSize, "accessibility-extra-extra-extra-large"),
            Change::Setting(Setting::IncreaseContrast, "on"),
            Change::Setting(Setting::ReduceMotion, "on"),
            Change::Setting(Setting::ButtonShapes, "on"),
        ],
    ),
    (
        "Late night in Tokyo",
        "Dark appearance, location Tokyo",
        &[
            Change::Setting(Setting::Appearance, "dark"),
            Change::Place(4),
        ],
    ),
    (
        "Color blindness",
        "Red/Green filter, Button Shapes",
        &[
            Change::Setting(Setting::ColorFilter, "red-green"),
            Change::Setting(Setting::ButtonShapes, "on"),
        ],
    ),
    (
        "Fresh install",
        "Every permission of the app in front back to Ask",
        &[Change::ResetPermissions],
    ),
];

/// Text sizes the screenshot matrix offers, by index into `TEXT_SIZES`.
const SHOT_SIZES: [usize; 5] = [2, 3, 6, 9, 11];
const KEPT_LOG_LINES: usize = 500;
/// How often logs are read while their tool has been opened.
const POLL: Duration = Duration::from_secs(3);
/// Settings change from outside too (agents, simctl); read them this often.
const SETTINGS_EVERY: u32 = 10;

#[derive(Default)]
struct Inputs {
    url: Option<Entity<Input>>,
    coords: Option<Entity<Input>>,
    payload: Option<Entity<Input>>,
    filter: Option<Entity<Input>>,
}

pub(super) struct Tools {
    pub(super) open: Option<Tool>,
    pub(super) pinned: Option<Tool>,
    /// The simulator the state below is for.
    device: Option<String>,
    settings: Option<Settings>,
    reading: bool,
    writes: usize,
    location: Option<(SharedString, f64, f64)>,
    app: Option<String>,
    looking_for_app: bool,
    grants: Option<BTreeMap<&'static str, Grant>>,
    /// Permissions changed here, for the app they belong to.
    changed_permissions: BTreeMap<Permission, (String, Grant)>,
    logs: VecDeque<LogLine>,
    log_seq: u64,
    logs_on: bool,
    unseen_errors: usize,
    shots: Vec<PathBuf>,
    shooting: Option<(usize, usize)>,
    shot_dark: (bool, bool),
    shot_sizes: BTreeSet<usize>,
    recording: Option<sim::Recording>,
    /// Overrides an agent made, marked until someone changes them.
    agent: BTreeSet<Override>,
    notice: Option<SharedString>,
    busy: BTreeSet<&'static str>,
    anchors: BTreeMap<Tool, AnchorSlot>,
    /// The device panel: presses in it leave a flyout open, so the phone
    /// can be used while a tool shows.
    pub(super) region: AnchorSlot,
    inputs: Inputs,
    poll: Option<Task<()>>,
    /// Shows the rail without a stream, for the headless screenshots.
    #[cfg(test)]
    demo: bool,
}

impl Default for Tools {
    fn default() -> Self {
        Self {
            open: None,
            pinned: None,
            device: None,
            settings: None,
            reading: false,
            writes: 0,
            location: None,
            app: None,
            looking_for_app: false,
            grants: None,
            changed_permissions: BTreeMap::new(),
            logs: VecDeque::new(),
            log_seq: 0,
            logs_on: false,
            unseen_errors: 0,
            shots: Vec::new(),
            shooting: None,
            shot_dark: (true, true),
            shot_sizes: [3, 9].into_iter().collect(),
            recording: None,
            agent: BTreeSet::new(),
            notice: None,
            busy: BTreeSet::new(),
            anchors: BTreeMap::new(),
            region: AnchorSlot::default(),
            inputs: Inputs::default(),
            poll: None,
            #[cfg(test)]
            demo: false,
        }
    }
}

impl Tools {
    fn visible(&self, tool: Tool) -> bool {
        self.open == Some(tool) || self.pinned == Some(tool)
    }

    /// What differs from a fresh simulator, in the order the strip shows it.
    pub(super) fn overrides(&self) -> Vec<(Override, String)> {
        let mut list = Vec::new();
        if let Some(settings) = &self.settings {
            for setting in Setting::ALL {
                if settings.changed(setting) {
                    let value = settings.get(setting).unwrap_or_default();
                    list.push((Override::Setting(setting), setting.describe(value)));
                }
            }
        }
        if let Some((name, _, _)) = &self.location {
            list.push((Override::Location, format!("Location {name}")));
        }
        for (permission, (_, grant)) in &self.changed_permissions {
            if *grant != Grant::Ask {
                list.push((
                    Override::Permission(*permission),
                    format!("{} {}", permission.label(), grant.label()),
                ));
            }
        }
        list
    }

    fn modified(&self, tool: Tool) -> bool {
        let changed = |setting| self.settings.as_ref().is_some_and(|s| s.changed(setting));
        match tool {
            Tool::Display => [
                Setting::Appearance,
                Setting::TextSize,
                Setting::ColorFilter,
                Setting::LiquidGlass,
            ]
            .into_iter()
            .any(changed),
            Tool::Accessibility => [
                Setting::ReduceMotion,
                Setting::IncreaseContrast,
                Setting::ReduceTransparency,
                Setting::VoiceOver,
                Setting::ButtonShapes,
            ]
            .into_iter()
            .any(changed),
            Tool::Location => self.location.is_some(),
            Tool::Permissions => self
                .changed_permissions
                .values()
                .any(|(_, grant)| *grant != Grant::Ask),
            _ => false,
        }
    }
}

impl DevicePanel {
    /// The simulator in the panel, if the tools can drive it.
    fn tool_device(&self) -> Option<String> {
        self.selected
            .as_ref()
            .filter(|d| d.platform == Platform::Ios && d.booted)
            .map(|d| d.id.clone())
    }

    pub(super) fn tools_available(&self) -> bool {
        #[cfg(test)]
        if self.tools.demo {
            return self.tool_device().is_some();
        }
        self.live.is_some() && self.tool_device().is_some()
    }

    /// Called once a simulator streams: reads its settings and starts the
    /// light polling the tools need. A different simulator starts afresh.
    pub(super) fn tools_attach(&mut self, cx: &mut Context<Self>) {
        let Some(udid) = self.tool_device() else {
            return;
        };
        if self.tools.device.as_deref() != Some(&udid) {
            let (anchors, region, inputs) = (
                std::mem::take(&mut self.tools.anchors),
                self.tools.region.clone(),
                std::mem::take(&mut self.tools.inputs),
            );
            self.tools = Tools {
                device: Some(udid),
                anchors,
                region,
                inputs,
                #[cfg(test)]
                demo: self.tools.demo,
                ..Tools::default()
            };
        }
        self.read_settings(cx);
        if self.tools.poll.is_none() {
            self.tools.poll = Some(cx.spawn(async move |this, cx| {
                let mut tick = 0u32;
                loop {
                    cx.background_executor().timer(POLL).await;
                    tick += 1;
                    let alive = this.update(cx, |this, cx| {
                        if !this.shown || !this.tools_available() {
                            return;
                        }
                        if this.tools.logs_on {
                            this.read_logs(cx);
                        }
                        if tick.is_multiple_of(SETTINGS_EVERY) {
                            this.read_settings(cx);
                        }
                    });
                    if alive.is_err() {
                        break;
                    }
                }
            }));
        }
    }

    /// Runs blocking simulator work in the background, then `done` with it.
    fn tool_work<T: Send + 'static>(
        &mut self,
        work: impl FnOnce(String) -> Result<T, String> + Send + 'static,
        done: impl FnOnce(&mut Self, Result<T, String>, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(udid) = self.tool_device() else {
            return;
        };
        let task = cx.background_executor().spawn(async move { work(udid) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                done(this, result, cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn tool_failed(&mut self, error: String) {
        self.tools.notice = Some(error.into());
    }

    fn read_settings(&mut self, cx: &mut Context<Self>) {
        if self.tools.reading || self.tools.writes > 0 {
            return;
        }
        self.tools.reading = true;
        let device = self.tools.device.clone();
        self.tool_work(
            |udid| sim::settings(&udid),
            move |this, result, _| {
                this.tools.reading = false;
                if this.tools.device != device || this.tools.writes > 0 {
                    return;
                }
                match result {
                    Ok(settings) => {
                        // A setting put back by hand is no longer the agent's.
                        this.tools.agent.retain(|o| match o {
                            Override::Setting(s) => settings.changed(*s),
                            _ => true,
                        });
                        this.tools.settings = Some(settings);
                    }
                    Err(error) => this.tool_failed(error),
                }
            },
            cx,
        );
    }

    /// Changes a setting, showing it as made at once.
    pub(super) fn set_setting(
        &mut self,
        setting: Setting,
        value: &'static str,
        agent: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(settings) = self.tools.settings.as_mut() {
            settings.0.insert(setting, value.to_owned());
        }
        if agent && value != setting.default_value() {
            self.tools.agent.insert(Override::Setting(setting));
        } else {
            self.tools.agent.remove(&Override::Setting(setting));
        }
        self.tools.writes += 1;
        self.tools.notice = None;
        self.tool_work(
            move |udid| sim::set(&udid, setting, value),
            |this, result, cx| {
                this.tools.writes -= 1;
                if let Err(error) = result {
                    this.tool_failed(error);
                    this.read_settings(cx);
                }
            },
            cx,
        );
        cx.notify();
    }

    pub(super) fn set_place(
        &mut self,
        place: Option<(SharedString, f64, f64)>,
        agent: bool,
        cx: &mut Context<Self>,
    ) {
        let at = place.as_ref().map(|(_, lat, lng)| (*lat, *lng));
        self.tools.location = place;
        if agent && at.is_some() {
            self.tools.agent.insert(Override::Location);
        } else {
            self.tools.agent.remove(&Override::Location);
        }
        self.tools.notice = None;
        self.tool_work(
            move |udid| sim::set_location(&*hub::ensure()?, &udid, at),
            |this, result, _| {
                if let Err(error) = result {
                    this.tools.location = None;
                    this.tool_failed(error);
                }
            },
            cx,
        );
        cx.notify();
    }

    fn find_app(&mut self, cx: &mut Context<Self>) {
        if self.tools.looking_for_app {
            return;
        }
        self.tools.looking_for_app = true;
        self.tool_work(
            |udid| sim::foreground_app(&*hub::ensure()?, &udid),
            |this, result, cx| {
                this.tools.looking_for_app = false;
                match result {
                    Ok(app) => {
                        let changed = this.tools.app != app;
                        this.tools.app = app;
                        if changed || this.tools.grants.is_none() {
                            this.tools.grants = None;
                            if this.tools.visible(Tool::Permissions) {
                                this.read_grants(cx);
                            }
                        }
                    }
                    Err(error) => this.tool_failed(error),
                }
            },
            cx,
        );
    }

    fn read_grants(&mut self, cx: &mut Context<Self>) {
        let Some(bundle) = self.tools.app.clone() else {
            return;
        };
        self.tool_work(
            move |udid| sim::permissions(&udid, &bundle),
            |this, result, _| match result {
                Ok(grants) => this.tools.grants = Some(grants),
                Err(error) => this.tool_failed(error),
            },
            cx,
        );
    }

    fn set_grant(&mut self, permission: Permission, grant: Grant, cx: &mut Context<Self>) {
        let Some(bundle) = self.tools.app.clone() else {
            return;
        };
        if let Some(grants) = self.tools.grants.as_mut() {
            grants.insert(permission.service(), grant);
        }
        self.tools
            .changed_permissions
            .insert(permission, (bundle.clone(), grant));
        self.tools.agent.remove(&Override::Permission(permission));
        self.tools.notice = None;
        self.tool_work(
            move |udid| sim::set_permission(&udid, &bundle, permission, grant),
            |this, result, cx| {
                if let Err(error) = result {
                    this.tool_failed(error);
                    this.read_grants(cx);
                }
            },
            cx,
        );
        cx.notify();
    }

    fn reset_permission(&mut self, permission: Permission, cx: &mut Context<Self>) {
        let Some((bundle, _)) = self.tools.changed_permissions.remove(&permission) else {
            return;
        };
        if self.tools.app.as_deref() == Some(&bundle)
            && let Some(grants) = self.tools.grants.as_mut()
        {
            grants.insert(permission.service(), Grant::Ask);
        }
        self.tool_work(
            move |udid| sim::set_permission(&udid, &bundle, permission, Grant::Ask),
            |this, result, _| {
                if let Err(error) = result {
                    this.tool_failed(error);
                }
            },
            cx,
        );
    }

    fn reset_all_permissions(&mut self, cx: &mut Context<Self>) {
        let Some(bundle) = self.tools.app.clone() else {
            self.tool_failed("Open an app on the phone first.".into());
            return;
        };
        self.tools.changed_permissions.clear();
        if let Some(grants) = self.tools.grants.as_mut() {
            for grant in grants.values_mut() {
                *grant = Grant::Ask;
            }
        }
        self.tool_work(
            move |udid| {
                hub::serve_sim_cli(&["permissions", "reset", "all", &bundle, "-q", "-d", &udid])
            },
            |this, result, _| {
                if let Err(error) = result {
                    this.tool_failed(error);
                }
            },
            cx,
        );
    }

    /// Puts back what differs, as the strip's × does.
    fn clear_override(&mut self, item: Override, cx: &mut Context<Self>) {
        self.tools.agent.remove(&item);
        match item {
            Override::Setting(setting) => {
                self.set_setting(setting, setting.default_value(), false, cx)
            }
            Override::Location => self.set_place(None, false, cx),
            Override::Permission(permission) => self.reset_permission(permission, cx),
        }
        cx.notify();
    }

    fn clear_all_overrides(&mut self, cx: &mut Context<Self>) {
        for (item, _) in self.tools.overrides() {
            self.clear_override(item, cx);
        }
    }

    fn app_action(&mut self, label: &'static str, cx: &mut Context<Self>) {
        let Some(bundle) = self.tools.app.clone() else {
            return;
        };
        let url = self
            .tools
            .inputs
            .url
            .as_ref()
            .map(|input| input.read(cx).content.trim().to_owned())
            .unwrap_or_default();
        if label == "open" && url.is_empty() {
            return;
        }
        self.tools.busy.insert(label);
        self.tools.notice = None;
        self.tool_work(
            move |udid| {
                let endpoint = hub::ensure()?;
                match label {
                    "restart" => sim::restart_app(&endpoint, &udid, &bundle),
                    "stop" => sim::stop_app(&endpoint, &udid, &bundle),
                    _ => sim::open_url(&endpoint, &udid, &bundle, &url),
                }
            },
            move |this, result, _| {
                this.tools.busy.remove(label);
                if let Err(error) = result {
                    this.tool_failed(error);
                }
            },
            cx,
        );
        cx.notify();
    }

    fn send_push(&mut self, cx: &mut Context<Self>) {
        let Some(bundle) = self.tools.app.clone() else {
            return;
        };
        let payload = self
            .tools
            .inputs
            .payload
            .as_ref()
            .map(|input| input.read(cx).content.to_string())
            .unwrap_or_default();
        self.tools.busy.insert("push");
        self.tools.notice = None;
        self.tool_work(
            move |udid| sim::push(&udid, &bundle, &payload),
            |this, result, _| {
                this.tools.busy.remove("push");
                match result {
                    Ok(()) => this.tools.notice = Some("Sent.".into()),
                    Err(error) => this.tool_failed(error),
                }
            },
            cx,
        );
        cx.notify();
    }

    fn set_custom_place(&mut self, cx: &mut Context<Self>) {
        let text = self
            .tools
            .inputs
            .coords
            .as_ref()
            .map(|input| input.read(cx).content.to_string())
            .unwrap_or_default();
        match sim::parse_coordinates(&text) {
            Some((lat, lng)) => self.set_place(
                Some((format!("{lat:.3}, {lng:.3}").into(), lat, lng)),
                false,
                cx,
            ),
            None => self.tool_failed("Type a latitude and longitude, like 42.69, 23.32.".into()),
        }
    }

    fn read_logs(&mut self, cx: &mut Context<Self>) {
        if self.tools.busy.contains("logs") {
            return;
        }
        self.tools.busy.insert("logs");
        let since = self.tools.log_seq;
        self.tool_work(
            move |udid| sim::logs(&*hub::ensure()?, &udid, since),
            |this, result, _| {
                this.tools.busy.remove("logs");
                let Ok(lines) = result else {
                    return;
                };
                let watching = this.tools.visible(Tool::Logs);
                for line in lines {
                    this.tools.log_seq = this.tools.log_seq.max(line.seq);
                    if line.error && !watching {
                        this.tools.unseen_errors += 1;
                    }
                    this.tools.logs.push_back(line);
                }
                while this.tools.logs.len() > KEPT_LOG_LINES {
                    this.tools.logs.pop_front();
                }
            },
            cx,
        );
    }

    fn apply_preset(&mut self, index: usize, cx: &mut Context<Self>) {
        for change in PRESETS[index].2 {
            match *change {
                Change::Setting(setting, value) => self.set_setting(setting, value, false, cx),
                Change::Place(place) => {
                    let (name, lat, lng) = PLACES[place];
                    self.set_place(Some((name.into(), lat, lng)), false, cx)
                }
                Change::ResetPermissions => self.reset_all_permissions(cx),
            }
        }
        self.tools.notice = Some(format!("Applied {}.", PRESETS[index].0).into());
        cx.notify();
    }

    /// Captures the screen in each chosen appearance and text size, then
    /// puts the simulator back as it was.
    fn capture_matrix(&mut self, cx: &mut Context<Self>) {
        let mut combos = Vec::new();
        for (dark, wanted) in [
            (false, self.tools.shot_dark.0),
            (true, self.tools.shot_dark.1),
        ] {
            if wanted {
                for size in &self.tools.shot_sizes {
                    combos.push((dark, *size));
                }
            }
        }
        if combos.is_empty() {
            return;
        }
        let before = self.tools.settings.clone().unwrap_or_default();
        self.tools.shooting = Some((0, combos.len()));
        self.tools.shots.clear();
        self.tools.notice = None;
        cx.notify();
        let Some(udid) = self.tool_device() else {
            return;
        };
        let (progress, updates) = async_channel::unbounded::<usize>();
        let task = cx.background_executor().spawn(async move {
            let mut shots = Vec::new();
            let result = (|| {
                for (ix, (dark, size)) in combos.iter().enumerate() {
                    let _ = progress.try_send(ix);
                    sim::set(
                        &udid,
                        Setting::Appearance,
                        if *dark { "dark" } else { "light" },
                    )?;
                    sim::set(&udid, Setting::TextSize, sim::TEXT_SIZES[*size])?;
                    // Apps redraw for a new size or appearance over a few frames.
                    std::thread::sleep(Duration::from_millis(1200));
                    let name = format!(
                        "{}-{}",
                        if *dark { "dark" } else { "light" },
                        sim::TEXT_SIZE_NAMES[*size].to_lowercase()
                    );
                    shots.push(sim::screenshot(&udid, &name)?);
                }
                Ok::<_, String>(())
            })();
            for setting in [Setting::Appearance, Setting::TextSize] {
                let value = before.get(setting).unwrap_or(setting.default_value());
                let _ = sim::set(&udid, setting, value);
            }
            result.map(|_| shots)
        });
        cx.spawn(async move |this, cx| {
            while let Ok(ix) = updates.recv().await {
                if this
                    .update(cx, |this, cx| {
                        if let Some((_, total)) = this.tools.shooting {
                            this.tools.shooting = Some((ix, total));
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    return;
                }
            }
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.tools.shooting = None;
                match result {
                    Ok(shots) => this.tools.shots = shots,
                    Err(error) => this.tool_failed(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn toggle_recording(&mut self, cx: &mut Context<Self>) {
        if let Some(recording) = self.tools.recording.take() {
            self.tools.busy.insert("record");
            let task = cx
                .background_executor()
                .spawn(async move { recording.stop() });
            cx.spawn(async move |this, cx| {
                let result = task.await;
                let _ = this.update(cx, |this, cx| {
                    this.tools.busy.remove("record");
                    match result {
                        Ok(path) => cx.emit(DeviceEvent::Attach(vec![path])),
                        Err(error) => this.tool_failed(error),
                    }
                    cx.notify();
                });
            })
            .detach();
        } else if let Some(udid) = self.tool_device() {
            match sim::start_recording(&udid) {
                Ok(recording) => {
                    self.tools.recording = Some(recording);
                    // The header's timer ticks once a second.
                    cx.spawn(async move |this, cx| {
                        loop {
                            cx.background_executor().timer(Duration::from_secs(1)).await;
                            let recording = this
                                .update(cx, |this, cx| {
                                    cx.notify();
                                    this.tools.recording.is_some()
                                })
                                .unwrap_or(false);
                            if !recording {
                                break;
                            }
                        }
                    })
                    .detach();
                }
                Err(error) => self.tool_failed(error),
            }
        }
        cx.notify();
    }

    /// Saves one screenshot and adds it to the message being written.
    pub(super) fn attach_screenshot(&mut self, cx: &mut Context<Self>) {
        self.tools.busy.insert("shot");
        self.tool_work(
            |udid| sim::screenshot(&udid, "screen"),
            |this, result, cx| {
                this.tools.busy.remove("shot");
                match result {
                    Ok(path) => cx.emit(DeviceEvent::Attach(vec![path])),
                    Err(error) => this.tool_failed(error),
                }
            },
            cx,
        );
        cx.notify();
    }

    pub(super) fn recording_label(&self) -> Option<SharedString> {
        let seconds = self.tools.recording.as_ref()?.started.elapsed().as_secs();
        Some(format!("{}:{:02}", seconds / 60, seconds % 60).into())
    }

    pub(super) fn recording_busy(&self) -> bool {
        self.tools.busy.contains("record")
    }

    pub(super) fn stop_recording(&mut self, cx: &mut Context<Self>) {
        if self.tools.recording.is_some() {
            self.toggle_recording(cx);
        }
    }

    pub(super) fn screenshot_busy(&self) -> bool {
        self.tools.busy.contains("shot")
    }

    /// Opens a tool's flyout, or closes it if it is open.
    pub(super) fn toggle_tool(&mut self, tool: Tool, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        if tool == Tool::Record {
            self.toggle_recording(cx);
            return;
        }
        if self.tools.open == Some(tool) {
            self.tools.open = None;
        } else {
            self.tools.open = Some(tool);
            self.tools.notice = None;
            self.prepare_tool(tool, window, cx);
        }
        cx.notify();
    }

    /// Reads what a tool shows, and makes its fields.
    fn prepare_tool(&mut self, tool: Tool, window: &mut Window, cx: &mut Context<Self>) {
        let placeholder = content(0.35);
        match tool {
            Tool::Display | Tool::Accessibility => self.read_settings(cx),
            Tool::App | Tool::Push | Tool::Permissions => {
                self.find_app(cx);
                if tool == Tool::Permissions {
                    self.read_grants(cx);
                }
            }
            Tool::Logs => {
                self.tools.logs_on = true;
                self.tools.unseen_errors = 0;
                self.read_logs(cx);
            }
            _ => {}
        }
        let inputs = &mut self.tools.inputs;
        match tool {
            Tool::App if inputs.url.is_none() => {
                inputs.url =
                    Some(cx.new(|cx| {
                        Input::field("URL to open", "myapp://orders/42", placeholder, cx)
                    }));
            }
            Tool::Location if inputs.coords.is_none() => {
                inputs.coords = Some(cx.new(|cx| {
                    Input::field(
                        "Latitude and longitude",
                        "42.6977, 23.3219",
                        placeholder,
                        cx,
                    )
                }));
            }
            Tool::Push if inputs.payload.is_none() => {
                let input = cx.new(|cx| {
                    let mut input = Input::multiline_field("Push payload", "{\"aps\": …}", cx);
                    input.set_text(PUSHES[0].1.to_owned(), cx);
                    input
                });
                inputs.payload = Some(input);
            }
            Tool::Logs if inputs.filter.is_none() => {
                let input = cx.new(|cx| Input::field("Filter logs", "Filter", placeholder, cx));
                cx.observe(&input, |_, _, cx| cx.notify()).detach();
                inputs.filter = Some(input);
            }
            _ => {}
        }
        let _ = window;
    }

    pub(super) fn tool_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.tools_available() {
            return;
        }
        let keystroke = &event.keystroke;
        if keystroke.key == "escape" && self.tools.open.is_some() {
            self.tools.open = None;
            cx.notify();
            cx.stop_propagation();
            return;
        }
        let modifiers = keystroke.modifiers;
        if modifiers.alt
            && !modifiers.platform
            && !modifiers.control
            && let Some(tool) = Tool::from_key(&keystroke.key)
        {
            self.toggle_tool(tool, window, cx);
            cx.stop_propagation();
        }
    }

    /// The column of tools beside the phone.
    pub(super) fn tool_rail(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let mut rail = div()
            .id("device-tool-rail")
            .debug_selector(|| "device-tool-rail".into())
            .role(Role::Toolbar)
            .aria_label("Device tools")
            .w(px(42.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(3.))
            .py_2()
            .border_r_1()
            .border_color(content(0.06));
        for entry in RAIL {
            rail = match entry {
                None => rail.child(div().w(px(18.)).h(px(1.)).my(px(4.)).bg(content(0.1))),
                Some(tool) => rail.child(self.rail_button(tool, cx)),
            };
        }
        rail.into_any_element()
    }

    fn rail_button(&mut self, tool: Tool, cx: &mut Context<Self>) -> AnyElement {
        let recording = tool == Tool::Record && self.tools.recording.is_some();
        let active = self.tools.visible(tool) || recording;
        let slot = self.tools.anchors.entry(tool).or_default().clone();
        let badge = (tool == Tool::Logs && self.tools.unseen_errors > 0)
            .then(|| self.tools.unseen_errors.min(99));
        let modified = self.tools.modified(tool);
        let tip = match tool.key() {
            Some(key) => format!("{}  ⌥{key}", tool.label()),
            None if recording => "Stop recording".into(),
            None => tool.label().into(),
        };
        let red: gpui::Hsla = rgb(0xf87171).into();
        div()
            .id(("device-tool", tool.index()))
            .debug_selector(move || format!("device-tool-{}", tool.index()))
            .role(Role::Button)
            .aria_label(tool.label())
            .tip(tip)
            .relative()
            .size(px(30.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(7.))
            .cursor_pointer()
            .when(active, |d| d.bg(content(0.1)))
            .when(!active, |d| d.hover(|s| s.bg(content(0.06))))
            .child(slot.probe())
            .child(icon(tool.icon(), 16.).text_color(if recording {
                red
            } else if active {
                content(0.95)
            } else {
                content(0.55)
            }))
            .when(modified, |d| {
                d.child(
                    div()
                        .absolute()
                        .bottom(px(3.))
                        .right(px(3.))
                        .size(px(6.))
                        .rounded_full()
                        .bg(accent_color()),
                )
            })
            .when_some(badge, |d, count| {
                d.child(
                    div()
                        .absolute()
                        .top(px(-1.))
                        .right(px(-2.))
                        .min_w(px(14.))
                        .h(px(14.))
                        .px(px(3.))
                        .rounded_full()
                        .bg(red)
                        .text_color(gpui::white())
                        .text_size(px(9.5))
                        .line_height(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .flex()
                        .justify_center()
                        .child(count.to_string()),
                )
            })
            .on_click(cx.listener(move |this, _, window, cx| this.toggle_tool(tool, window, cx)))
            .into_any_element()
    }

    /// The open tool, over the conversation beside the panel.
    pub(super) fn tool_flyout(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let tool = self.tools.open.filter(|_| self.tools_available())?;
        let anchor = self.tools.anchors.get(&tool)?.clone();
        let this = cx.entity().downgrade();
        let body = self.tool_body(tool, window, cx);
        Some(
            popover::popover(("device-tool-flyout", tool.index()), Anchor::Slot(anchor))
                .side(Side::Left)
                .align(Align::Start)
                .gap(10.)
                .width(300.)
                .max_height(620.)
                .ignore(self.tools.region.clone())
                .on_dismiss(move |_, cx| {
                    let _ = this.update(cx, |panel, cx| {
                        panel.tools.open = None;
                        cx.notify();
                    });
                })
                .child(self.tool_title(tool, false, cx))
                .child(
                    div()
                        .id("device-tool-body")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .px(px(12.))
                        .pt(px(10.))
                        .pb(px(12.))
                        .child(body),
                )
                .into_any_element(),
        )
    }

    fn tool_title(&self, tool: Tool, pinned: bool, cx: &mut Context<Self>) -> AnyElement {
        div()
            .h(px(36.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_1()
            .pl(px(12.))
            .pr(px(6.))
            .border_b_1()
            .border_color(content(0.06))
            .child(
                div()
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(content(0.9))
                    .child(tool.label()),
            )
            .when_some(tool.key().filter(|_| !pinned), |d, key| {
                d.child(
                    div()
                        .ml(px(6.))
                        .px(px(4.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(content(0.12))
                        .text_size(px(10.5))
                        .text_color(content(0.5))
                        .child(format!("⌥{key}")),
                )
            })
            .child(div().flex_1())
            .when(tool.pinnable(), |d| {
                let (label, path) = if pinned {
                    ("Unpin", "pin-off")
                } else {
                    ("Pin under the phone", "pin")
                };
                d.child(
                    crate::right_panel::icon_action(
                        ("device-tool-pin", tool.index()),
                        label,
                        path,
                        true,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if pinned {
                            this.tools.pinned = None;
                        } else {
                            this.tools.pinned = Some(tool);
                            this.tools.open = None;
                        }
                        cx.notify();
                    })),
                )
            })
            .when(!pinned, |d| {
                d.child(
                    crate::right_panel::icon_action(
                        ("device-tool-close", tool.index()),
                        "Close",
                        "x",
                        true,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tools.open = None;
                        cx.notify();
                    })),
                )
            })
            .into_any_element()
    }

    /// The pinned tool, under the phone.
    pub(super) fn tool_dock(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let tool = self.tools.pinned.filter(|_| self.tools_available())?;
        let body = self.tool_body(tool, window, cx);
        Some(
            div()
                .h(px(250.))
                .flex_shrink_0()
                .flex()
                .flex_col()
                .border_t_1()
                .border_color(content(0.08))
                .child(self.tool_title(tool, true, cx))
                .child(
                    div()
                        .id("device-tool-dock")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .px(px(12.))
                        .py(px(10.))
                        .child(body),
                )
                .into_any_element(),
        )
    }

    /// What differs from a fresh simulator, each with a way to put it back.
    pub(super) fn overrides_strip(&self, cx: &mut Context<Self>) -> AnyElement {
        let overrides = self.tools.overrides();
        let strip = div()
            .id("device-overrides")
            .debug_selector(|| "device-overrides".into())
            .flex_shrink_0()
            .min_h(px(42.))
            .px(px(10.))
            .py(px(8.))
            .border_t_1()
            .border_color(content(0.06))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.));
        if overrides.is_empty() {
            let text = if self.tools.settings.is_none() {
                "Reading the simulator's settings…"
            } else {
                "No overrides. The simulator is as it booted."
            };
            return strip
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(content(0.45))
                        .child(text),
                )
                .into_any_element();
        }
        strip
            .children(
                overrides
                    .into_iter()
                    .enumerate()
                    .map(|(ix, (item, label))| {
                        let agent = self.tools.agent.contains(&item);
                        div()
                            .id(("device-override", ix))
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .h(px(24.))
                            .pl(px(8.))
                            .pr(px(4.))
                            .rounded_full()
                            .text_size(px(11.5))
                            .text_color(content(0.85))
                            .map(|d| {
                                if agent {
                                    d.border_1().border_dashed().border_color(accent_color())
                                } else {
                                    d.bg(content(0.08))
                                }
                            })
                            .when(agent, |d| {
                                d.tip("Set by an agent")
                                    .child(icon("bot", 11.).text_color(accent_color()))
                            })
                            .child(label.clone())
                            .child(
                                div()
                                    .id(("device-override-clear", ix))
                                    .role(Role::Button)
                                    .aria_label(SharedString::from(format!("Clear {label}")))
                                    .size(px(16.))
                                    .rounded_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .hover(|s| s.bg(content(0.12)))
                                    .child(icon("x", 10.).text_color(content(0.6)))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.clear_override(item, cx)
                                    })),
                            )
                    }),
            )
            .child(div().flex_1())
            .child(
                text_button("device-overrides-reset", "Reset all")
                    .on_click(cx.listener(|this, _, _, cx| this.clear_all_overrides(cx))),
            )
            .into_any_element()
    }

    fn tool_body(&mut self, tool: Tool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let notice = self.tools.notice.clone();
        let body = match tool {
            Tool::Display => self.display_body(cx),
            Tool::Accessibility => self.accessibility_body(cx),
            Tool::App => self.app_body(cx),
            Tool::Location => self.location_body(cx),
            Tool::Permissions => self.permissions_body(cx),
            Tool::Push => self.push_body(cx),
            Tool::Logs => self.logs_body(cx),
            Tool::Presets => self.presets_body(cx),
            Tool::Shots => self.shots_body(cx),
            Tool::Record => div().into_any_element(),
        };
        let _ = window;
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(body)
            .when_some(notice, |d, notice| {
                d.child(
                    div()
                        .mt(px(4.))
                        .text_size(px(11.5))
                        .line_height(px(16.))
                        .text_color(content(0.6))
                        .child(notice),
                )
            })
            .into_any_element()
    }

    fn setting(&self, setting: Setting) -> Option<String> {
        self.tools
            .settings
            .as_ref()
            .and_then(|s| s.get(setting))
            .map(str::to_owned)
    }

    fn display_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(_) = &self.tools.settings else {
            return loading("Reading the simulator's settings…");
        };
        let appearance = self.setting(Setting::Appearance).unwrap_or_default();
        let size = self.setting(Setting::TextSize).unwrap_or_default();
        let size_ix = sim::TEXT_SIZES.iter().position(|s| *s == size).unwrap_or(3);
        let filter = self.setting(Setting::ColorFilter).unwrap_or_default();
        let glass = self.setting(Setting::LiquidGlass);
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(row(
                "Appearance",
                segmented(
                    "device-appearance",
                    &[("light", "Light"), ("dark", "Dark")],
                    &appearance,
                    cx,
                    |this, value, cx| this.set_setting(Setting::Appearance, value, false, cx),
                ),
            ))
            .child(row(
                format!("Text size · {}", sim::TEXT_SIZE_NAMES[size_ix]),
                notches(size_ix, cx),
            ))
            .child(
                div()
                    .pt(px(6.))
                    .text_size(px(12.))
                    .text_color(content(0.8))
                    .child("Color filter"),
            )
            .child(div().flex().flex_wrap().gap(px(6.)).pt(px(4.)).children(
                sim::COLOR_FILTERS.iter().map(|value| {
                    let value: &'static str = value;
                    pill(
                        SharedString::from(format!("device-filter-{value}")),
                        sim::color_filter_name(value),
                        filter == value,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_setting(Setting::ColorFilter, value, false, cx)
                    }))
                }),
            ))
            .when_some(glass, |d, glass| {
                d.child(div().h(px(6.))).child(row(
                    "Liquid Glass",
                    segmented(
                        "device-glass",
                        &[("clear", "Clear"), ("tinted", "Tinted")],
                        &glass,
                        cx,
                        |this, value, cx| this.set_setting(Setting::LiquidGlass, value, false, cx),
                    ),
                ))
            })
            .into_any_element()
    }

    fn accessibility_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.tools.settings.is_none() {
            return loading("Reading the simulator's settings…");
        }
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .children(
                [
                    Setting::ReduceMotion,
                    Setting::IncreaseContrast,
                    Setting::ReduceTransparency,
                    Setting::ButtonShapes,
                    Setting::VoiceOver,
                ]
                .into_iter()
                .filter(|setting| self.setting(*setting).is_some())
                .map(|setting| {
                    let on = self.setting(setting).as_deref() == Some("on");
                    row(
                        setting.label(),
                        switch(("device-switch", setting as usize), setting.label(), on)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_setting(setting, if on { "off" } else { "on" }, false, cx)
                            }))
                            .into_any_element(),
                    )
                }),
            )
            .into_any_element()
    }

    /// The app in front, or a note saying there is none.
    fn app_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let looking = self.tools.looking_for_app;
        let line = match &self.tools.app {
            Some(bundle) => div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(12.))
                .text_color(content(0.85))
                .font_family(crate::fonts::mono())
                .child(bundle.clone()),
            None => div()
                .flex_1()
                .text_size(px(12.))
                .text_color(content(0.55))
                .child(if looking {
                    "Looking for the app in front…"
                } else {
                    "No app is in front. Open one, or answer the alert on the phone."
                }),
        };
        div()
            .flex()
            .items_center()
            .gap_2()
            .min_h(px(24.))
            .child(line)
            .child(
                crate::right_panel::icon_action(
                    "device-app-refresh",
                    "Look again",
                    "refresh-cw",
                    !looking,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.tools.grants = None;
                    this.find_app(cx);
                    cx.notify();
                })),
            )
            .into_any_element()
    }

    fn app_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let has_app = self.tools.app.is_some();
        let busy = |label| self.tools.busy.contains(label);
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(self.app_header(cx))
            .when(has_app, |d| {
                d.child(
                    div()
                        .flex()
                        .gap(px(6.))
                        .child(
                            button("device-app-restart", "Restart", busy("restart")).on_click(
                                cx.listener(|this, _, _, cx| this.app_action("restart", cx)),
                            ),
                        )
                        .child(
                            button("device-app-stop", "Stop", busy("stop")).on_click(
                                cx.listener(|this, _, _, cx| this.app_action("stop", cx)),
                            ),
                        ),
                )
                .when_some(self.tools.inputs.url.clone(), |d, url| {
                    d.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(field(url))
                            .child(button("device-app-open", "Open", busy("open")).on_click(
                                cx.listener(|this, _, _, cx| this.app_action("open", cx)),
                            )),
                    )
                })
            })
            .into_any_element()
    }

    fn location_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let current = self
            .tools
            .location
            .as_ref()
            .map(|(name, _, _)| name.clone());
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .children(PLACES.iter().enumerate().map(|(ix, (name, lat, lng))| {
                        let (name, lat, lng) = (*name, *lat, *lng);
                        pill(("device-place", ix), name, current.as_deref() == Some(name)).on_click(
                            cx.listener(move |this, _, _, cx| {
                                if this
                                    .tools
                                    .location
                                    .as_ref()
                                    .is_some_and(|(n, _, _)| n == name)
                                {
                                    this.set_place(None, false, cx)
                                } else {
                                    this.set_place(Some((name.into(), lat, lng)), false, cx)
                                }
                            }),
                        )
                    })),
            )
            .when_some(self.tools.inputs.coords.clone(), |d, coords| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(field(coords))
                        .child(
                            button("device-place-set", "Set", false)
                                .on_click(cx.listener(|this, _, _, cx| this.set_custom_place(cx))),
                        ),
                )
            })
            .when_some(current, |d, name| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(11.5))
                                .text_color(content(0.55))
                                .child(format!("Simulating {name}")),
                        )
                        .child(
                            text_button("device-place-clear", "Stop simulating").on_click(
                                cx.listener(|this, _, _, cx| this.set_place(None, false, cx)),
                            ),
                        ),
                )
            })
            .into_any_element()
    }

    fn permissions_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let header = self.app_header(cx);
        let Some(_) = &self.tools.app else {
            return header;
        };
        let Some(grants) = self.tools.grants.clone() else {
            return div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(header)
                .child(loading("Reading its permissions…"))
                .into_any_element();
        };
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(header)
            .children(Permission::ALL.into_iter().map(|permission| {
                let grant = grants
                    .get(permission.service())
                    .copied()
                    .unwrap_or(Grant::Ask);
                let current = grant.label();
                row(
                    permission.label(),
                    segmented(
                        match permission {
                            Permission::Camera => "device-perm-camera",
                            Permission::Photos => "device-perm-photos",
                            Permission::Location => "device-perm-location",
                            Permission::Notifications => "device-perm-notifications",
                            Permission::Microphone => "device-perm-microphone",
                            Permission::Contacts => "device-perm-contacts",
                        },
                        &[("Allow", "Allow"), ("Deny", "Deny"), ("Ask", "Ask")],
                        current,
                        cx,
                        move |this, value, cx| {
                            let grant = match value {
                                "Allow" => Grant::Allowed,
                                "Deny" => Grant::Denied,
                                _ => Grant::Ask,
                            };
                            this.set_grant(permission, grant, cx)
                        },
                    ),
                )
            }))
            .child(
                div().pt(px(6.)).child(
                    text_button("device-perm-reset", "Reset all to Ask")
                        .on_click(cx.listener(|this, _, _, cx| this.reset_all_permissions(cx))),
                ),
            )
            .into_any_element()
    }

    fn push_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let header = self.app_header(cx);
        let Some(_) = &self.tools.app else {
            return header;
        };
        let sending = self.tools.busy.contains("push");
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(header)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .children(PUSHES.iter().enumerate().map(|(ix, (label, payload))| {
                        let payload: &'static str = payload;
                        pill(("device-push-preset", ix), label, false).on_click(cx.listener(
                            move |this, _, _, cx| {
                                if let Some(input) = this.tools.inputs.payload.clone() {
                                    input.update(cx, |input, cx| {
                                        input.set_text(payload.to_owned(), cx)
                                    });
                                }
                            },
                        ))
                    })),
            )
            .when_some(self.tools.inputs.payload.clone(), |d, payload| {
                d.child(
                    div()
                        .rounded(px(6.))
                        .border_1()
                        .border_color(content(0.1))
                        .px(px(8.))
                        .py(px(6.))
                        .font_family(crate::fonts::mono())
                        .child(payload),
                )
            })
            .child(
                div().flex().justify_end().child(
                    crate::settings::widgets::primary_button("device-push-send", "Send", sending)
                        .child(if sending { "Sending…" } else { "Send" })
                        .when(!sending, |d| {
                            d.on_click(cx.listener(|this, _, _, cx| this.send_push(cx)))
                        }),
                ),
            )
            .into_any_element()
    }

    fn logs_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let filter = self
            .tools
            .inputs
            .filter
            .as_ref()
            .map(|input| input.read(cx).content.to_lowercase())
            .unwrap_or_default();
        let lines: Vec<LogLine> = self
            .tools
            .logs
            .iter()
            .rev()
            .filter(|line| {
                filter.is_empty()
                    || line.message.to_lowercase().contains(&filter)
                    || line.process.to_lowercase().contains(&filter)
            })
            .take(200)
            .cloned()
            .collect();
        let empty = lines.is_empty();
        let red: gpui::Hsla = rgb(0xf87171).into();
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .when_some(self.tools.inputs.filter.clone(), |d, input| {
                        d.child(field(input))
                    })
                    .child(
                        text_button("device-logs-clear", "Clear").on_click(cx.listener(
                            |this, _, _, cx| {
                                this.tools.logs.clear();
                                cx.notify();
                            },
                        )),
                    ),
            )
            .when(empty, |d| {
                d.child(
                    div()
                        .py_2()
                        .text_size(px(11.5))
                        .text_color(content(0.5))
                        .child("Waiting for your apps to log. Newest lines show first."),
                )
            })
            .children(lines.into_iter().map(|line| {
                div()
                    .flex()
                    .gap(px(6.))
                    .font_family(crate::fonts::mono())
                    .text_size(px(10.5))
                    .line_height(px(15.))
                    .when(!line.process.is_empty(), |d| {
                        d.child(
                            div()
                                .flex_shrink_0()
                                .max_w(px(80.))
                                .truncate()
                                .text_color(content(0.4))
                                .child(line.process.clone()),
                        )
                    })
                    .child(
                        div()
                            .min_w_0()
                            .text_color(if line.error { red } else { content(0.78) })
                            .child(line.message.clone()),
                    )
            }))
            .into_any_element()
    }

    fn presets_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .children(PRESETS.iter().enumerate().map(|(ix, (name, detail, _))| {
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .p(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(content(0.08))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(content(0.9))
                                    .child(*name),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .line_height(px(15.))
                                    .text_color(content(0.5))
                                    .child(*detail),
                            ),
                    )
                    .child(
                        button(("device-preset", ix), "Apply", false)
                            .on_click(cx.listener(move |this, _, _, cx| this.apply_preset(ix, cx))),
                    )
            }))
            .into_any_element()
    }

    fn shots_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let (light, dark) = self.tools.shot_dark;
        let count = (light as usize + dark as usize) * self.tools.shot_sizes.len();
        let shooting = self.tools.shooting;
        let shots = self.tools.shots.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(11.5))
                    .line_height(px(16.))
                    .text_color(content(0.55))
                    .child(
                        "Captures the screen in each combination, then puts the simulator back.",
                    ),
            )
            .child(row(
                "Appearance",
                div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        pill("device-shot-light", "Light", light).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.tools.shot_dark.0 = !this.tools.shot_dark.0;
                                cx.notify();
                            },
                        )),
                    )
                    .child(pill("device-shot-dark", "Dark", dark).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.tools.shot_dark.1 = !this.tools.shot_dark.1;
                            cx.notify();
                        },
                    )))
                    .into_any_element(),
            ))
            .child(row(
                "Text size",
                div()
                    .flex()
                    .gap(px(4.))
                    .children(SHOT_SIZES.into_iter().map(|size| {
                        let on = self.tools.shot_sizes.contains(&size);
                        pill(("device-shot-size", size), sim::TEXT_SIZE_NAMES[size], on).on_click(
                            cx.listener(move |this, _, _, cx| {
                                if !this.tools.shot_sizes.remove(&size) {
                                    this.tools.shot_sizes.insert(size);
                                }
                                cx.notify();
                            }),
                        )
                    }))
                    .into_any_element(),
            ))
            .child(
                div().flex().justify_end().child(match shooting {
                    Some((ix, total)) => div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_size(px(12.))
                        .text_color(content(0.6))
                        .child(crate::right_panel::spinner(
                            "device-shot-spinner",
                            "loader",
                            14.,
                        ))
                        .child(format!("Capturing {} of {total}…", ix + 1))
                        .into_any_element(),
                    None => crate::settings::widgets::primary_button(
                        "device-shot-capture",
                        "Capture",
                        count == 0,
                    )
                    .child(format!(
                        "Capture {count} screenshot{}",
                        if count == 1 { "" } else { "s" }
                    ))
                    .when(count > 0, |d| {
                        d.on_click(cx.listener(|this, _, _, cx| this.capture_matrix(cx)))
                    })
                    .into_any_element(),
                }),
            )
            .when(!shots.is_empty(), |d| {
                let attach = shots.clone();
                d.child(div().flex().flex_wrap().gap(px(8.)).children(
                    shots.iter().enumerate().map(|(ix, path)| {
                        let name = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .and_then(|s| s.rsplit_once('-').map(|(name, _)| name.to_owned()))
                            .unwrap_or_default();
                        div()
                            .id(("device-shot", ix))
                            .w(px(80.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                img(path.clone())
                                    .w(px(80.))
                                    .h(px(172.))
                                    .rounded(px(8.))
                                    .object_fit(ObjectFit::Contain),
                            )
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(content(0.5))
                                    .child(name.replace('-', " · ")),
                            )
                    }),
                ))
                .child(div().flex().justify_end().child(
                    button("device-shot-attach", "Add to the message", false).on_click(
                        cx.listener(move |this, _, _, cx| {
                            cx.emit(DeviceEvent::Attach(attach.clone()));
                            this.tools.open = None;
                            cx.notify();
                        }),
                    ),
                ))
            })
            .into_any_element()
    }

    /// An agent changed something on the simulator through MonoCode; the
    /// strip marks it as the agent's until someone changes it.
    pub(crate) fn agent_set(
        &mut self,
        setting: Option<(Setting, &'static str)>,
        place: Option<Option<(SharedString, f64, f64)>>,
        cx: &mut Context<Self>,
    ) {
        if let Some((setting, value)) = setting {
            if let Some(settings) = self.tools.settings.as_mut() {
                settings.0.insert(setting, value.to_owned());
            }
            if value != setting.default_value() {
                self.tools.agent.insert(Override::Setting(setting));
            } else {
                self.tools.agent.remove(&Override::Setting(setting));
            }
        }
        if let Some(place) = place {
            if place.is_some() {
                self.tools.agent.insert(Override::Location);
            } else {
                self.tools.agent.remove(&Override::Location);
            }
            self.tools.location = place;
        }
        cx.notify();
    }
}

fn loading(text: &'static str) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .py_1()
        .text_size(px(12.))
        .text_color(content(0.55))
        .child(crate::right_panel::spinner(text, "loader", 14.))
        .child(text)
        .into_any_element()
}

fn row(label: impl Into<SharedString>, control: impl IntoElement) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .min_h(px(30.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(px(12.))
                .text_color(content(0.8))
                .child(label.into()),
        )
        .child(control)
        .into_any_element()
}

fn switch(id: impl Into<gpui::ElementId>, label: &'static str, on: bool) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .role(Role::Switch)
        .aria_label(label)
        .aria_toggled(if on {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .relative()
        .h(px(18.))
        .w(px(30.))
        .flex_shrink_0()
        .rounded_full()
        .cursor_pointer()
        .bg(if on { accent_color() } else { content(0.2) })
        .child(
            div()
                .absolute()
                .top(px(2.))
                .left(px(if on { 14. } else { 2. }))
                .size(px(14.))
                .rounded_full()
                .bg(gpui::white()),
        )
}

fn segmented(
    id: &'static str,
    options: &[(&'static str, &'static str)],
    current: &str,
    cx: &mut Context<DevicePanel>,
    change: impl Fn(&mut DevicePanel, &'static str, &mut Context<DevicePanel>) + Clone + 'static,
) -> AnyElement {
    div()
        .id(id)
        .role(Role::RadioGroup)
        .flex()
        .flex_shrink_0()
        .gap(px(2.))
        .p(px(2.))
        .rounded(px(7.))
        .bg(content(0.06))
        .children(options.iter().map(|&(value, label)| {
            let selected = current == value;
            let change = change.clone();
            div()
                .id(SharedString::from(format!("{id}-{value}")))
                .role(Role::RadioButton)
                .aria_label(label)
                .aria_selected(selected)
                .h(px(22.))
                .px(px(8.))
                .flex()
                .items_center()
                .rounded(px(5.))
                .text_size(px(11.5))
                .cursor_pointer()
                .map(|d| {
                    if selected {
                        d.bg(content(0.12)).text_color(content(1.))
                    } else {
                        d.text_color(content(0.6))
                            .hover(|s| s.text_color(content(1.)))
                    }
                })
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !selected {
                        change(this, value, cx)
                    }
                }))
        }))
        .into_any_element()
}

/// Twelve steps of text size, filled up to the current one.
fn notches(current: usize, cx: &mut Context<DevicePanel>) -> AnyElement {
    div()
        .id("device-text-size")
        .role(Role::Slider)
        .aria_label("Text size")
        .w(px(132.))
        .h(px(18.))
        .flex()
        .items_center()
        .gap(px(2.))
        .children((0..sim::TEXT_SIZES.len()).map(|ix| {
            let value: &'static str = sim::TEXT_SIZES[ix];
            div()
                .id(("device-text-size-step", ix))
                .tip(sim::TEXT_SIZE_NAMES[ix])
                .flex_1()
                .h(px(if ix == current { 14. } else { 8. }))
                .rounded(px(2.))
                .cursor_pointer()
                .bg(if ix <= current {
                    accent_color()
                } else {
                    content(0.14)
                })
                .hover(|s| s.opacity(0.75))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.set_setting(Setting::TextSize, value, false, cx)
                }))
        }))
        .into_any_element()
}

fn pill(id: impl Into<gpui::ElementId>, label: &str, on: bool) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(SharedString::from(label.to_owned()))
        .aria_selected(on)
        .h(px(24.))
        .px(px(9.))
        .flex()
        .items_center()
        .rounded_full()
        .text_size(px(11.5))
        .cursor_pointer()
        .map(|d| {
            if on {
                d.bg(accent_color().opacity(0.22)).text_color(content(1.))
            } else {
                d.border_1()
                    .border_color(content(0.14))
                    .text_color(content(0.8))
                    .hover(|s| s.bg(content(0.06)))
            }
        })
        .child(label.to_owned())
}

fn button(id: impl Into<gpui::ElementId>, label: &'static str, busy: bool) -> Stateful<gpui::Div> {
    crate::settings::widgets::secondary_button(id, label, false, busy).child(label)
}

fn text_button(id: &'static str, label: &'static str) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .h(px(24.))
        .px(px(6.))
        .flex()
        .items_center()
        .rounded(px(5.))
        .text_size(px(11.5))
        .text_color(content(0.6))
        .cursor_pointer()
        .hover(|s| s.bg(content(0.06)).text_color(content(1.)))
        .child(label)
}

fn field(input: Entity<Input>) -> AnyElement {
    div()
        .flex_1()
        .min_w_0()
        .h(px(28.))
        .px(px(8.))
        .flex()
        .items_center()
        .rounded(px(6.))
        .border_1()
        .border_color(content(0.12))
        .child(input)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinates_take_commas_or_spaces_within_range() {
        assert_eq!(sim::parse_coordinates("42.69, 23.32"), Some((42.69, 23.32)));
        assert_eq!(
            sim::parse_coordinates(" -33.8 151.2 "),
            Some((-33.8, 151.2))
        );
        assert_eq!(sim::parse_coordinates("91, 0"), None);
        assert_eq!(sim::parse_coordinates("1, 2, 3"), None);
        assert_eq!(sim::parse_coordinates("Sofia"), None);
    }

    #[test]
    fn shortcuts_open_the_first_eight_tools() {
        assert_eq!(Tool::from_key("1"), Some(Tool::Display));
        assert_eq!(Tool::from_key("7"), Some(Tool::Logs));
        assert_eq!(Tool::from_key("8"), Some(Tool::Presets));
        assert_eq!(Tool::from_key("9"), None);
    }

    #[test]
    fn overrides_list_what_differs_from_a_fresh_simulator() {
        let mut tools = Tools {
            settings: Some(
                Settings::parse(
                    r#"{"appearance":"dark","text-size":"large","reduce-motion":"on","color-filter":"none"}"#,
                )
                .unwrap(),
            ),
            location: Some(("Tokyo".into(), 35.6, 139.6)),
            ..Tools::default()
        };
        tools
            .changed_permissions
            .insert(Permission::Camera, ("b".into(), Grant::Denied));
        tools
            .changed_permissions
            .insert(Permission::Photos, ("b".into(), Grant::Ask));
        let labels: Vec<String> = tools.overrides().into_iter().map(|(_, l)| l).collect();
        assert_eq!(
            labels,
            ["Dark", "Reduce Motion", "Location Tokyo", "Camera Deny"]
        );
        assert!(tools.modified(Tool::Display));
        assert!(tools.modified(Tool::Accessibility));
        assert!(tools.modified(Tool::Location));
        assert!(tools.modified(Tool::Permissions));
        assert!(!tools.modified(Tool::App));
    }

    struct Root(Entity<DevicePanel>);
    impl gpui::Render for Root {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            // A stand-in conversation, so flyouts show where they open.
            div()
                .size_full()
                .flex()
                .bg(rgb(0x141414))
                .text_color(rgb(0xebebeb))
                .child(
                    div()
                        .flex_1()
                        .p_6()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .text_size(px(13.))
                        .child(
                            div()
                                .self_end()
                                .max_w(px(360.))
                                .p_3()
                                .rounded(px(12.))
                                .bg(content(0.08))
                                .child("Check the order screen with large text and dark mode."),
                        )
                        .child(div().text_color(content(0.75)).child(
                            "I'll switch the simulator to dark mode and AX3 text, then take a look.",
                        )),
                )
                .child(
                    div()
                        .w(px(440.))
                        .h_full()
                        .border_l_1()
                        .border_color(content(0.08))
                        .child(self.0.clone()),
                )
        }
    }

    /// Renders the rail and every tool against the first booted simulator,
    /// using the real hub: each tool reads (and the scripted steps change)
    /// that simulator. Writes PNGs to `DEVICE_TOOL_SHOTS`.
    #[test]
    #[ignore = "Requires a Metal device and a booted simulator; changes its settings"]
    fn device_tools_shots() {
        use gpui::{HeadlessAppContext, size};
        use std::sync::Arc;
        let dir =
            std::env::var("DEVICE_TOOL_SHOTS").unwrap_or_else(|_| "/tmp/device-tool-shots".into());
        std::fs::create_dir_all(&dir).unwrap();
        monocode_core::device::set_enabled(true, false);
        hub::ensure().expect("the hub starts");
        let device = hub::list()
            .unwrap()
            .devices
            .into_iter()
            .find(|d| d.platform == Platform::Ios && d.booted)
            .expect("a booted simulator");
        let mut cx = HeadlessAppContext::with_platform(
            Arc::new(gpui_apple::AppleTextSystem::new()),
            Arc::new(crate::assets::Assets),
            || {
                Ok(Some(Box::new(
                    gpui_apple::metal_renderer::MetalHeadlessRenderer::new(),
                )))
            },
        );
        cx.update(crate::popover::bind);
        cx.update(crate::input::bind);
        let mut panel = None;
        let window = cx
            .open_window(size(px(1100.), px(800.)), |_, cx| {
                let entity = cx.new(DevicePanel::new);
                panel = Some(entity.clone());
                cx.new(|_| Root(entity))
            })
            .unwrap();
        let panel = panel.unwrap();
        let settle = |cx: &mut HeadlessAppContext, until: &dyn Fn(&DevicePanel) -> bool| {
            let started = std::time::Instant::now();
            loop {
                std::thread::sleep(std::time::Duration::from_millis(100));
                cx.run_until_parked();
                cx.update_window(window.into(), |_, w, _| w.refresh())
                    .unwrap();
                cx.run_until_parked();
                let done = cx.update(|cx| until(panel.read(cx)));
                if done || started.elapsed() > std::time::Duration::from_secs(20) {
                    break;
                }
            }
            for _ in 0..3 {
                std::thread::sleep(std::time::Duration::from_millis(120));
                cx.run_until_parked();
                cx.update_window(window.into(), |_, w, _| w.refresh())
                    .unwrap();
                cx.run_until_parked();
            }
        };
        let shot = |cx: &mut HeadlessAppContext, name: &str| {
            cx.capture_screenshot(window.into())
                .unwrap()
                .save(format!("{dir}/{name}.png"))
                .unwrap();
        };
        let stage =
            |cx: &mut HeadlessAppContext,
             f: &dyn Fn(&mut DevicePanel, &mut Window, &mut Context<DevicePanel>)| {
                window
                    .update(cx, |_, window, cx| {
                        panel.update(cx, |p, cx| {
                            f(p, window, cx);
                            cx.notify();
                        })
                    })
                    .unwrap();
            };
        stage(&mut cx, &|p, _, cx| {
            p.shown = true;
            p.hub = super::super::HubState::Ready;
            p.selected = Some(device.clone());
            p.tools.demo = true;
            p.tools_attach(cx);
        });
        settle(&mut cx, &|p| p.tools.settings.is_some());
        shot(&mut cx, "01-rail");
        for (ix, tool) in [
            Tool::Display,
            Tool::Accessibility,
            Tool::App,
            Tool::Location,
            Tool::Permissions,
            Tool::Push,
            Tool::Logs,
            Tool::Presets,
            Tool::Shots,
        ]
        .into_iter()
        .enumerate()
        {
            stage(&mut cx, &|p, w, cx| p.toggle_tool(tool, w, cx));
            settle(&mut cx, &|p| {
                !p.tools.reading
                    && !p.tools.looking_for_app
                    && (tool != Tool::Permissions
                        || p.tools.app.is_none()
                        || p.tools.grants.is_some())
            });
            shot(
                &mut cx,
                &format!(
                    "{:02}-{}",
                    ix + 2,
                    tool.label().to_lowercase().replace(' ', "-")
                ),
            );
        }
        // Change things and read them back from the simulator.
        stage(&mut cx, &|p, w, cx| {
            p.toggle_tool(Tool::Display, w, cx);
            p.set_setting(Setting::Appearance, "dark", false, cx);
            p.set_setting(Setting::TextSize, "accessibility-extra-large", false, cx);
            p.set_place(Some(("Tokyo".into(), 35.6762, 139.6503)), true, cx);
        });
        settle(&mut cx, &|p| p.tools.writes == 0);
        stage(&mut cx, &|p, _, cx| p.read_settings(cx));
        settle(&mut cx, &|p| !p.tools.reading);
        let read = cx
            .update(|cx| panel.read(cx).tools.settings.clone())
            .unwrap();
        assert_eq!(read.get(Setting::Appearance), Some("dark"));
        assert_eq!(
            read.get(Setting::TextSize),
            Some("accessibility-extra-large")
        );
        shot(&mut cx, "11-changed");
        let phone = sim::screenshot(&device.id, "check").unwrap();
        std::fs::copy(&phone, format!("{dir}/11-phone.png")).unwrap();
        stage(&mut cx, &|p, _, cx| {
            p.tools.pinned = Some(Tool::Accessibility);
            p.tools.open = None;
            cx.notify();
        });
        settle(&mut cx, &|_| true);
        shot(&mut cx, "12-pinned");
        stage(&mut cx, &|p, _, cx| p.clear_all_overrides(cx));
        settle(&mut cx, &|p| p.tools.writes == 0);
        stage(&mut cx, &|p, _, cx| p.read_settings(cx));
        settle(&mut cx, &|p| !p.tools.reading);
        let read = cx
            .update(|cx| panel.read(cx).tools.settings.clone())
            .unwrap();
        assert_eq!(read.get(Setting::Appearance), Some("light"));
        assert_eq!(read.get(Setting::TextSize), Some("large"));
        shot(&mut cx, "13-reset");
    }
}
