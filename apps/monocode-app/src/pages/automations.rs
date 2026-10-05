//! Persisted automations, template drafts, and manual run controls.
use crate::adapters::{automations::AppAutomationsData, projects_data::AppProjectsData};
use gpui::{AnyView, App, AppContext as _, Window};
use monocode_engine::automations::AutomationsPackage;
use monocode_view_pages::automations::AutomationsView;
use std::rc::Rc;

pub fn page(window: &mut Window, cx: &mut App) -> Option<AnyView> {
    let workspace = crate::slots::window_workspace_for(window, cx)?;
    let cwd = workspace.read(cx).sidebar_cwd(cx);
    let view = crate::slots::cached_view("automations", window, cx, |window, cx| {
        let data = Rc::new(AppAutomationsData {
            automations: AutomationsPackage::try_global(cx)?.automations.clone(),
            workspace,
        });
        let projects = Rc::new(AppProjectsData::new(cx)?);
        Some(
            cx.new(|cx| AutomationsView::new(data, projects, Some(&cwd), window, cx))
                .into(),
        )
    })?;
    if let Ok(automations) = view.clone().downcast::<AutomationsView>() {
        automations.update(cx, |automations, cx| automations.set_cwd(Some(&cwd), cx));
    }
    Some(view)
}
