//! Devices: iOS Simulators and Android Emulators in the right panel, and
//! whether agents may drive them.
use crate::{
    device,
    settings::{
        SettingsView,
        widgets::{RED, note, secondary_button},
    },
    theme::*,
};
use gpui::{AnyElement, Context, SharedString, prelude::*};
use monocode_core::device::toolchain::Tool;

impl SettingsView {
    pub(super) fn devices_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if !cfg!(target_os = "macos") {
            return vec![self.group(
                Some("devices"),
                true,
                "Devices",
                None,
                None,
                vec![note("Devices need the macOS app.")],
            )];
        }
        let p = self.prefs.clone();
        if device::setup(cx).is_none_or(|s| s.doctor.is_none()) {
            device::check(cx);
        }
        let setup = device::setup(cx);
        let doctor = setup.and_then(|s| s.doctor.clone());
        let installing = setup.and_then(|s| s.installing);
        let error = setup.and_then(|s| s.error.clone());

        let switches = vec![
            self.row(
                Some("devices"),
                true,
                "Simulators and emulators",
                Some(
                    "Adds a Device tab to the right panel that shows a running iOS Simulator or Android Emulator, which you can tap and type into. Streams with the Device Hub, which runs on your Node.js and is installed the first time."
                        .into(),
                ),
                vec![self.toggle("devices", "Simulators and emulators", p.devices, false, cx, |this, on, cx| {
                    this.update(cx, |p| p.devices = on);
                    if on {
                        device::install(&[Tool::Hub], cx);
                    }
                })],
            ),
            self.row(
                Some("device-agents"),
                false,
                "Agent access",
                Some(
                    "Lets agents open a device, take screenshots, and tap, type and swipe with agent-device. What they do shows in the Device tab."
                        .into(),
                ),
                vec![self.toggle(
                    "device-agents",
                    "Agent access",
                    p.device_agents,
                    !p.devices,
                    cx,
                    |this, on, cx| {
                        this.update(cx, |p| p.device_agents = on);
                        if on {
                            device::install(&[Tool::Hub, Tool::AgentDevice], cx);
                        }
                    },
                )],
            ),
        ];

        let mac = match &doctor {
            None => vec![note("Checking this Mac…")],
            Some(doctor) => {
                let node: SharedString = match &doctor.node {
                    Some((path, version)) if doctor.node_supported() => {
                        format!("{version} at {}.", path.display()).into()
                    }
                    Some((path, version)) => format!(
                        "{version} at {} is too old. Agent access needs Node.js 22.12 or newer.",
                        path.display()
                    )
                    .into(),
                    None => "Not found. Install Node.js 22.12 or newer from nodejs.org or with Homebrew.".into(),
                };
                let android: SharedString = match &doctor.android_sdk {
                    Some(path) => format!("At {}.", path.display()).into(),
                    None => "Not found. Install Android Studio for Android Emulators.".into(),
                };
                vec![
                    self.row(None, true, "Node.js", Some(node), Vec::new()),
                    self.row(
                        None,
                        false,
                        "Xcode",
                        Some(if doctor.xcode {
                            "Installed. iOS Simulators are available.".into()
                        } else {
                            "Not found. Install Xcode for iOS Simulators.".into()
                        }),
                        Vec::new(),
                    ),
                    self.row(None, false, "Android SDK", Some(android), Vec::new()),
                ]
            }
        };
        let recheck = secondary_button("devices-recheck", "Check again", false, false)
            .child(icon("refresh-cw", 14.))
            .child("Check again")
            .on_click(cx.listener(|_, _, _, cx| device::check(cx)))
            .into_any_element();

        let mut tools: Vec<AnyElement> = Tool::ALL
            .iter()
            .enumerate()
            .map(|(index, &tool)| {
                let description: SharedString = if installing == Some(tool) {
                    format!("Installing version {}…", tool.version()).into()
                } else if tool.installed() {
                    format!("Version {}.", tool.version()).into()
                } else {
                    "Not installed.".into()
                };
                self.row(
                    None,
                    index == 0,
                    tool.label(),
                    Some(description),
                    Vec::new(),
                )
            })
            .collect();
        if let Some(error) = error {
            tools.push(note(error));
        }
        let any_installed = Tool::ALL.iter().any(|tool| tool.installed());
        let action = if installing.is_some() {
            None
        } else if any_installed {
            Some(
                secondary_button("devices-remove", "Remove tools", true, false)
                    .child(icon("trash", 14.).text_color(gpui::rgb(RED)))
                    .child("Remove")
                    .on_click(cx.listener(|_, _, _, cx| device::remove(cx)))
                    .into_any_element(),
            )
        } else if p.devices {
            Some(
                secondary_button("devices-install", "Install", false, false)
                    .child("Install")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let tools: &[Tool] = if this.prefs.device_agents {
                            &[Tool::Hub, Tool::AgentDevice]
                        } else {
                            &[Tool::Hub]
                        };
                        device::install(tools, cx);
                    }))
                    .into_any_element(),
            )
        } else {
            None
        };

        vec![
            self.group(None, true, "Devices", None, None, switches),
            self.group(None, false, "This Mac", None, Some(recheck), mac),
            self.group(
                None,
                false,
                "Tools",
                Some("Installed with npm into MonoCode's data folder, at pinned versions.".into()),
                action,
                tools,
            ),
        ]
    }
}
