//! Notes with the history package's persistence, image assets, and chat action.
use crate::adapters::{notes::AppNotesData, projects_data::AppProjectsData};
use gpui::{AnyView, App, AppContext as _, Window};
use monocode_app::boot::AppServices;
use monocode_app::bridge::shell::{ShellPage, ShellRequest, ShellRequests};
use monocode_engine::history::HistoryPackage;
use monocode_view_pages::notes::NotesView;
use std::rc::Rc;

pub fn page(window: &mut Window, cx: &mut App) -> Option<AnyView> {
    let workspace = crate::slots::window_workspace_for(window, cx)?;
    let cwd = workspace.read(cx).sidebar_cwd(cx);
    let view = crate::slots::cached_view("notes", window, cx, |window, cx| {
        let data = Rc::new(AppNotesData {
            notes: HistoryPackage::try_global(cx)?.notes.clone(),
            data_dir: AppServices::global(cx).data_dir.path.clone(),
        });
        let projects = Rc::new(AppProjectsData::new(cx)?);
        Some(
            cx.new(|cx| {
                NotesView::new(data, projects, Some(&cwd), window, cx).on_close(|_, cx| {
                    ShellRequests::send(ShellRequest::ClosePage(ShellPage::Notes), cx);
                })
            })
            .into(),
        )
    })?;
    if let Ok(notes) = view.clone().downcast::<NotesView>() {
        notes.update(cx, |notes, cx| notes.set_cwd(Some(&cwd), cx));
    }
    Some(view)
}
