use crate::settings::SettingsView;
use crate::{
    settings::{SettingsEvent, management::Target, widgets::icon_action},
    theme::*,
};
use gpui::{AnyElement, Context, SharedString, div, prelude::*, px};
impl SettingsView {
    pub(super) fn monos_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let p = self.prefs.clone();
        let mut rows=vec![
            self.row(Some("monos-enabled"),true,"Show monos",Some("Agents of your own on the project rail. Each works on the projects you give it, remembers what matters and picks up habits it runs on its own. Turn this off to hide them.".into()),vec![self.toggle("monos-enabled","Show monos",p.monos,false,cx,|this,on,cx|this.update(cx,|p|p.monos=on))]),
            self.row(Some("rail-monos-pinned"),false,"Pin monos to the icon rail",Some("When the project rail is collapsed to icons, show each Mono at the top of the rail, above a divider, instead of inside the project picker.".into()),vec![self.toggle("rail-monos-pinned","Pin monos to the icon rail",p.rail_monos_pinned,!p.monos,cx,|this,on,cx|this.update(cx,|p|p.rail_monos_pinned=on))])];
        if cfg!(target_os = "macos") {
            rows.push(self.row(Some("mono-menu-bar-icon"),false,"Menu bar icon",Some("Chat with a Mono or open the quick composer from the macOS menu bar. Turn this off to hide the icon.".into()),vec![self.toggle("mono-menu-bar-icon","Menu bar icon",p.mono_menu_bar_icon,false,cx,|this,on,cx|this.update(cx,|p|p.mono_menu_bar_icon=on))]));
        }
        let mut roster = Vec::new();
        for (ix, mono) in self.ctx.monos.clone().into_iter().enumerate() {
            let show_id = mono.id.clone();
            let show = mono.show;
            let folders = mono.folders;
            let description = match mono.projects.split_last() {
                None => "No projects yet".to_owned(),
                Some((last, [])) => format!("Works on {last}"),
                Some((last, others)) => format!("Works on {} and {last}", others.join(", ")),
            };
            let label = div()
                .min_w_0()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .debug_selector(|| "settings-mono-mascot".into())
                        .size(px(16.))
                        .flex_shrink_0()
                        .child(crate::monos::avatar::avatar(
                            &mono.mascot,
                            mono.color,
                            "idle",
                            16.,
                            true,
                        )),
                )
                .child(
                    div()
                        .debug_selector(|| "settings-mono-name".into())
                        .min_w_0()
                        .truncate()
                        .child(mono.name.clone()),
                );
            let reset = mono.clone();
            let controls = vec![
                div()
                    .text_size(px(12.))
                    .line_height(px(20.))
                    .text_color(content(0.5))
                    .child("Show Mono spawned session on the sidebar")
                    .into_any_element(),
                div()
                    .debug_selector(|| "settings-mono-sidebar-switch".into())
                    .flex_shrink_0()
                    .child(self.toggle(
                        "mono-sidebar-show",
                        format!("Show sessions started by {} in sidebar", mono.name),
                        show,
                        false,
                        cx,
                        move |_, on, cx| {
                            cx.emit(SettingsEvent::MonoSidebar(show_id.clone(), on, folders))
                        },
                    ))
                    .into_any_element(),
                icon_action(
                    "reset-mono",
                    format!("Reset {} to defaults", mono.name),
                    "rotate-ccw",
                    28.,
                    14.,
                    0.4,
                    false,
                )
                .debug_selector(|| "settings-mono-reset".into())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_management(Target::ResetMono(reset.clone()), window, cx)
                }))
                .into_any_element(),
            ];
            roster.push(
                div()
                    .id(SharedString::from(format!("settings-mono-{}", mono.id)))
                    .child(self.row(None, ix == 0, label, Some(description.into()), controls))
                    .into_any_element(),
            );
        }
        if roster.is_empty() {
            roster.push(super::super::widgets::note("No monos yet."));
        }
        vec![self.group(None,true,"Monos",None,None,rows),self.group(Some("mono-list"),false,"Your monos",Some("Show or hide sessions started by each Mono. Hidden sessions remain available from the Mono's chat.".into()),None,roster)]
    }
}
