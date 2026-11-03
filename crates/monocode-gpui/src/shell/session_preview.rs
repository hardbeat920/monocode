//! Hover cards for unopened sessions do not construct conversation entities.
use crate::{pickers::cached_model_name, theme::*};
use gpui::{AnyElement, FontWeight, div, prelude::*, px};
use monocode_core::{
    provider::Kind,
    store::{Session, SessionPreviewData},
};
use std::path::Path;

pub(super) fn render(
    summary: &Session,
    data: Option<&SessionPreviewData>,
    done: bool,
) -> AnyElement {
    let model = data
        .and_then(|data| data.model.as_deref())
        .and_then(|model| monocode_core::store::native_model(model, &summary.provider));
    let name = cached_model_name(Kind::from_id(&summary.provider), model.as_deref());
    let worktree = data.and_then(|data| data.worktree_cwd.as_deref());
    let cwd = worktree.unwrap_or(&summary.cwd);
    let directory = std::env::var_os("HOME")
        .and_then(|home| {
            Path::new(cwd).strip_prefix(home).ok().map(|path| {
                if path.as_os_str().is_empty() {
                    "~".to_owned()
                } else {
                    format!("~/{}", path.display())
                }
            })
        })
        .unwrap_or_else(|| cwd.to_owned());
    div()
        .px_3()
        .py_2p5()
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .text_size(px(11.))
                        .text_color(content(0.55))
                        .child(provider_icon(&summary.provider, 14.))
                        .child(div().min_w_0().truncate().child(name)),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(11.))
                        .text_color(if done {
                            gpui::rgb(0x34d399).into()
                        } else {
                            content(0.45)
                        })
                        .child(if done {
                            "Done".to_owned()
                        } else {
                            monocode_core::time::relative(
                                summary.updated_at,
                                monocode_core::time::now_ms(),
                            )
                        }),
                ),
        )
        .child(
            div()
                .mt_1p5()
                .text_size(px(13.))
                .line_height(px(20.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(content(1.))
                .child(
                    monocode_core::session_title::display_title(&summary.title, &summary.provider)
                        .replace('\n', " "),
                ),
        )
        .child(
            div()
                .mt_2()
                .pt_2()
                .border_t_1()
                .border_color(content(0.08))
                .text_size(px(11.))
                .line_height(px(16.))
                .flex()
                .gap_2()
                .child(
                    div()
                        .w(px(72.))
                        .flex_shrink_0()
                        .text_color(content(0.45))
                        .child(if worktree.is_some() {
                            "Worktree"
                        } else {
                            "Directory"
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_color(content(0.8))
                        .child(directory),
                ),
        )
        .into_any_element()
}
