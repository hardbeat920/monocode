//! `UserMessageBlock` from AgentTranscript.tsx: the prompt bubble, its
//! attached context chips and files, the draft controls, the CI context
//! disclosure, and the actions under it.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div, img, px, relative,
};
use monocode_core::AttachmentKind;
use monocode_core::appearance::TranscriptLayout;
use monocode_core::block::SecondOpinionKind;
use monocode_core::transcript::BlockRef;
use monocode_core::{Attachment, Block};
use monocode_ui::styled::UiStyled as _;
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, file_type_icon, folder_type_icon, icon, u};

use crate::transcript::model::chat_context::{
    ChatContextItem, chip_label, file_target, split_chat_context,
};
use crate::transcript::model::link::{UserLink, parse_user_message_link};
use crate::transcript::model::plan::Row;
use crate::transcript::model::support::{
    is_operator_user_turn, operator_user_prompt, visible_user_prompt,
};
use crate::transcript::model::turn::format_clock_time;

use super::blocks::text_overflows;
use super::parts::harness_icon;
use super::style::{BUBBLE_MAX_WIDTH, TextSizes as _, palette};
use super::{TranscriptEvent, TranscriptView, eid};

impl TranscriptView {
    pub(super) fn render_user_message(
        &mut self,
        row: &Row,
        block: &BlockRef,
        can_edit: bool,
        editing: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = row.key.as_str();
        let theme = Theme::of(cx).clone();
        let card = block.second_opinion.clone();
        let note = block.note_card.clone();
        let monocode = is_operator_user_turn(block);
        let prompt_text = if monocode {
            operator_user_prompt(block)
        } else {
            block.text.clone()
        };
        let prompt = if card
            .as_ref()
            .is_some_and(|card| card.kind != Some(SecondOpinionKind::Handoff))
        {
            crate::transcript::model::chat_context::ChatContextMessage {
                text: String::new(),
                items: Vec::new(),
            }
        } else {
            split_chat_context(&visible_user_prompt(&prompt_text))
        };
        let text = prompt.text.clone();
        let context = prompt.items.clone();
        let link = if text.is_empty() {
            None
        } else {
            parse_user_message_link(&text)
        };
        let display_text = match &link {
            Some(link) => format!("{}{}", link.before_text, link.after_text),
            None => text.clone(),
        };
        let chat = self.config.layout == TranscriptLayout::Chat;
        let attachments = block.attachments.clone().unwrap_or_default();
        let draft = block.is_draft();
        let text_only = !text.is_empty()
            && !draft
            && attachments.is_empty()
            && context.is_empty()
            && card.is_none()
            && note.is_none()
            && block.ci_context.is_none();

        // Measure against the room the bubble's text has.
        let rem = window.rem_size();
        let column = self.column_width(window);
        let text_width = if chat {
            column.min(u(BUBBLE_MAX_WIDTH).to_pixels(rem)) - u(56. + 16. + 24.).to_pixels(rem)
        } else {
            column - u(12. + 24.).to_pixels(rem)
        };
        let expand_key = format!("user-expand:{}", block.id);
        let expanded = self.toggled(&expand_key, false) || row.search_current;
        let overflows =
            !display_text.is_empty() && text_overflows(&display_text, 4, 14., text_width, window);
        let single_line =
            chat && text_only && !text_overflows(&display_text, 1, 14., text_width, window);

        let accent = theme.colors.user_accent;
        let (fill, border) = match (draft, accent) {
            (true, Some(accent)) => (with(accent, 0.08), Some(with(accent, 0.38))),
            (true, None) => (theme.content(0.04), Some(theme.content(0.3))),
            (false, Some(accent)) => (with(accent, 0.24), Some(with(accent, 0.30))),
            (false, None) => (theme.content(0.10), (!chat).then(|| theme.content(0.10))),
        };
        let radius = if !chat {
            u(8.)
        } else if single_line {
            u(9999.)
        } else {
            u(12.)
        };
        let mut bubble = div()
            .relative()
            .min_w_0()
            .px(u(12.))
            .py(u(8.))
            .font_family(theme.fonts.sans.clone())
            .text_color(theme.colors.content)
            .bg(fill)
            .rounded(radius)
            .when_some(border, |el, border| {
                el.border_1()
                    .border_color(border)
                    .when(draft, |el| el.border_dashed())
            })
            .when(editing, |el| {
                el.border_1()
                    .border_dashed()
                    .border_color(with(theme.user_accent_or_accent(), 0.45))
            })
            .when(monocode, |el| {
                el.border_1()
                    .border_color(with(palette::amber_300_80(), 0.4))
            })
            .map(|el| {
                if chat {
                    el.max_w(u(BUBBLE_MAX_WIDTH))
                } else {
                    el.w_full()
                }
            });

        if !context.is_empty() || !attachments.is_empty() {
            let mut chips = div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(u(6.))
                .when(!text.is_empty() || card.is_some() || note.is_some(), |el| {
                    el.mb(u(8.))
                });
            for (index, item) in context.iter().enumerate() {
                chips = chips.child(self.render_context_chip(key, index, item, cx));
            }
            for (index, file) in attachments.iter().enumerate() {
                chips = chips.child(attachment_chip(key, index, file, &theme));
            }
            bubble = bubble.child(chips);
        }
        if let Some(note) = &note {
            bubble = bubble.child(
                div()
                    .when(!text.is_empty() || card.is_some(), |el| el.mb(u(8.)))
                    .rounded(u(6.))
                    .border_1()
                    .border_color(theme.content(0.1))
                    .bg(theme.content(0.06))
                    .px(u(10.))
                    .py(u(8.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(u(6.))
                            .child(
                                icon(IconName::StickyNote)
                                    .size(u(14.))
                                    .text_color(theme.content(0.45)),
                            )
                            .child(
                                div()
                                    .text_px(11.)
                                    .text_color(theme.content(0.5))
                                    .child("Note"),
                            ),
                    )
                    .child(div().mt(u(4.)).truncate().text_px(13.).semibold().child(
                        if note.title.is_empty() {
                            "Untitled".to_string()
                        } else {
                            note.title.clone()
                        },
                    )),
            );
        }
        if let Some(card) = &card {
            let files = card.files.filter(|files| *files > 0);
            bubble = bubble.child(
                div()
                    .min_w_0()
                    .when(!text.is_empty(), |el| el.mb(u(6.)))
                    .child(div().text_px(13.).medium().child(
                        if card.kind == Some(SecondOpinionKind::Handoff) {
                            "Handoff"
                        } else {
                            "Second opinion"
                        },
                    ))
                    .child(
                        div()
                            .mt(u(4.))
                            .flex()
                            .min_w_0()
                            .items_center()
                            .gap(u(6.))
                            .text_px(11.)
                            .text_color(theme.content(0.5))
                            .child(harness_icon(card.from))
                            .child(div().truncate().child(card.from.title()))
                            .child(
                                icon(IconName::ChevronRight)
                                    .size(u(12.))
                                    .text_color(theme.content(0.35)),
                            )
                            .child(harness_icon(card.to))
                            .child(div().truncate().child(card.to.title())),
                    )
                    .when_some(files, |el, files| {
                        el.child(
                            div()
                                .mt(u(4.))
                                .text_px(11.)
                                .text_color(theme.content(0.45))
                                .child(format!(
                                    "{files} {}",
                                    if files == 1 { "file" } else { "files" }
                                )),
                        )
                    }),
            );
        }
        if let Some(link) = &link {
            let chip = self.render_link_chip(key, &link.link, cx);
            // GPUI cannot flow an element inside wrapped text, so the chip
            // sits on its own line between the text around it.
            let before = monocode_core::js::trim_end(&link.before_text).to_string();
            let after = monocode_core::js::trim(&link.after_text).to_string();
            bubble = bubble.child(
                div()
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap(u(2.))
                    .min_w_0()
                    .text_sm_ui()
                    .when(!before.is_empty(), |el| {
                        el.child(div().w_full().min_w_0().child(before))
                    })
                    .child(chip)
                    .when(!after.is_empty(), |el| {
                        el.child(div().w_full().min_w_0().child(after))
                    }),
            );
        } else if !display_text.is_empty() {
            bubble = bubble.child(
                div()
                    .min_w_0()
                    .text_sm_ui()
                    .when(!expanded, |el| el.line_clamp(4))
                    .child(search_highlighted(
                        &display_text,
                        self.search_query(),
                        row.search_current,
                        &theme,
                    )),
            );
        }
        if overflows && link.is_none() {
            let toggle_key = expand_key.clone();
            bubble =
                bubble.child(
                    div()
                        .id(eid(key, "expand"))
                        .mt(u(4.))
                        .w_auto()
                        .rounded(u(4.))
                        .px(u(4.))
                        .py(u(2.))
                        .text_xs_ui()
                        .text_color(theme.content(0.6))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.content(0.08)).text_color(theme.colors.content))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.toggle(toggle_key.clone(), false, cx)
                        }))
                        .child(if expanded { "Show less" } else { "Show more" }),
                );
        }
        if let Some(ci) = &block.ci_context {
            let toggle = format!("ci:{}", block.id);
            let open = self.toggled(&toggle, false);
            let toggle_key = toggle.clone();
            bubble = bubble.child(
                div()
                    .mt(u(8.))
                    .min_w_0()
                    .border_t(px(1.))
                    .border_color(theme.content(0.1))
                    .pt(u(8.))
                    .child(
                        div()
                            .id(eid(key, &toggle))
                            .flex()
                            .items_center()
                            .gap(u(6.))
                            .text_xs_ui()
                            .text_color(theme.content(0.5))
                            .cursor_pointer()
                            .hover(|s| s.text_color(theme.content(0.8)))
                            .on_click(cx.listener(move |this, _, _, cx| this.toggle(toggle_key.clone(), false, cx)))
                            .child(
                                icon(if open { IconName::ChevronDown } else { IconName::ChevronRight })
                                    .size(u(12.))
                                    .text_color(theme.content(0.5)),
                            )
                            .child("CI context"),
                    )
                    .when(open, |el| {
                        el.child(
                            div()
                                .mt(u(8.))
                                .text_xs_ui()
                                .text_color(theme.content(0.5))
                                .child("CI instructions and failure details included with this request."),
                        )
                        .child(
                            div()
                                .id(eid(key, "ci-body"))
                                .mt(u(8.))
                                .max_h(u(288.))
                                .overflow_y_scroll()
                                .rounded(u(6.))
                                .bg(theme.content(0.05))
                                .p(u(10.))
                                .font_family(theme.fonts.mono.clone())
                                .text_px(11.)
                                .line_height(relative(1.625))
                                .text_color(theme.content(0.7))
                                .child(ci.clone()),
                        )
                    }),
            );
        }
        if draft {
            bubble = bubble.child(self.render_draft_controls(key, block, cx));
        }

        // The actions under the bubble show on hover.
        let mut actions = div()
            .flex()
            .items_center()
            .gap(u(4.))
            .px(u(12.))
            .pt(u(4.))
            .absolute()
            .top_full()
            .right_0()
            .opacity(0.)
            .group_hover("user-row", |s| s.opacity(1.));
        let has_actions =
            !text.is_empty() || !attachments.is_empty() || block.started_at.is_some() || can_edit;
        if !text.is_empty() || !attachments.is_empty() {
            let copy_key = format!("copy-user:{}", block.id);
            let copied = self.flashed(&copy_key);
            let copy_text = text.clone();
            actions = actions.child(
                self.action_button(
                    key,
                    "copy",
                    if copied {
                        IconName::Check
                    } else {
                        IconName::Copy
                    },
                    if copied { "Copied" } else { "Copy message" },
                    cx,
                )
                .relative()
                .left(u(-4.))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.copy(copy_key.clone(), copy_text.clone(), cx);
                })),
            );
        }
        if can_edit {
            let label = if editing {
                "Cancel edit"
            } else {
                "Edit and resend"
            };
            let button = self
                .action_button(key, "edit", IconName::Pencil, label, cx)
                .on_click(cx.listener(|_, _, _, cx| {
                    cx.stop_propagation();
                    cx.emit(TranscriptEvent::EditLastTurn);
                }));
            let accent = theme.user_accent_or_accent();
            actions = actions.child(if editing {
                button.bg(with(accent, 0.1))
            } else {
                button
            });
        }
        if !text.is_empty() && self.config.can_save_notes {
            let save_key = format!("note-user:{}", block.id);
            let saved = self.flashed(&save_key);
            let note_text = text.clone();
            actions = actions.child(
                self.action_button(
                    key,
                    "save",
                    if saved {
                        IconName::Check
                    } else {
                        IconName::FilePlusCorner
                    },
                    if saved {
                        "Saved to Notes"
                    } else {
                        "Save as note"
                    },
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.emit(TranscriptEvent::SaveNote {
                        text: note_text.clone(),
                    });
                    this.flash(save_key.clone(), cx);
                })),
            );
        }
        if let Some(started) = block.started_at {
            actions = actions.child(
                div()
                    .ml(u(4.))
                    .font_family(theme.fonts.sans.clone())
                    .text_xs_ui()
                    .text_color(theme.content(0.4))
                    .child(format_clock_time(started)),
            );
        }
        let zone = div()
            .relative()
            .min_w_0()
            .map(|el| {
                if chat {
                    el.flex().flex_col().items_end().max_w_full()
                } else {
                    el.w_full()
                }
            })
            .child(bubble)
            .when(has_actions, |el| el.child(actions));
        div()
            .id(eid(key, "user-row"))
            .group("user-row")
            .map(|el| {
                if chat {
                    el.flex()
                        .flex_col()
                        .items_end()
                        .pt(u(6.))
                        .pr(u(16.))
                        .pb(u(20.))
                        .pl(u(56.))
                } else {
                    el.p(u(6.)).pb(u(16.))
                }
            })
            .child(zone)
            .into_any_element()
    }

    /// The draft footer: Remove and Send.
    fn render_draft_controls(
        &mut self,
        key: &str,
        block: &Block,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let remove_id = block.id.clone();
        let send_id = block.id.clone();
        let enabled = self.config.can_send_drafts;
        div()
            .mt(u(8.))
            .flex()
            .items_center()
            .justify_between()
            .gap(u(16.))
            .border_t(px(1.))
            .border_dashed()
            .border_color(theme.content(0.2))
            .pt(u(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(6.))
                    .text_xs_ui()
                    .text_color(theme.content(0.5))
                    .child(
                        icon(IconName::CircleDashed)
                            .size(u(14.))
                            .text_color(theme.content(0.5)),
                    )
                    .child("Draft"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(4.))
                    .child(
                        div()
                            .id(eid(key, "remove-draft"))
                            .flex()
                            .items_center()
                            .gap(u(6.))
                            .h(u(28.))
                            .px(u(8.))
                            .rounded(u(6.))
                            .text_xs_ui()
                            .text_color(theme.content(0.55))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.content(0.1)).text_color(theme.colors.content))
                            .tooltip(tooltip("Remove draft"))
                            .when(enabled, |el| {
                                el.on_click(cx.listener(move |_, _, _, cx| {
                                    cx.emit(TranscriptEvent::RemoveDraft {
                                        block_id: remove_id.clone(),
                                    })
                                }))
                            })
                            .child(
                                icon(IconName::Trash2)
                                    .size(u(14.))
                                    .text_color(theme.content(0.55)),
                            )
                            .child("Remove"),
                    )
                    .child(
                        div()
                            .id(eid(key, "send-draft"))
                            .flex()
                            .items_center()
                            .gap(u(6.))
                            .h(u(28.))
                            .px(u(10.))
                            .rounded(u(6.))
                            .bg(theme.colors.primary)
                            .text_xs_ui()
                            .medium()
                            .text_color(theme.colors.primary_foreground)
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.colors.primary_hover))
                            .tooltip(tooltip("Send draft"))
                            .when(enabled, |el| {
                                el.on_click(cx.listener(move |_, _, _, cx| {
                                    cx.emit(TranscriptEvent::SendDraft {
                                        block_id: send_id.clone(),
                                    })
                                }))
                            })
                            .child("Send")
                            .child(
                                icon(IconName::ArrowUp)
                                    .size(u(14.))
                                    .text_color(theme.colors.primary_foreground),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// `ChatContextChip` as sent: a quote, a code selection, or a review
    /// comment. Code and comment chips open their file.
    fn render_context_chip(
        &mut self,
        key: &str,
        index: usize,
        item: &ChatContextItem,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let label = chip_label(item);
        let target = file_target(item);
        let glyph: AnyElement = match item {
            ChatContextItem::Code { path, .. } => file_type_icon(
                crate::transcript::model::chat_context::context_file_name(path),
            )
            .size(14.)
            .into_any_element(),
            ChatContextItem::Quote { .. } => icon(IconName::TextQuote)
                .size(u(14.))
                .text_color(theme.content(0.45))
                .into_any_element(),
            ChatContextItem::Comment { .. } => icon(IconName::MessageSquare)
                .size(u(14.))
                .text_color(theme.content(0.45))
                .into_any_element(),
        };
        let line_tag = label.line_tag.clone().map(|tag| {
            div()
                .flex_none()
                .font_family(theme.fonts.mono.clone())
                .text_px(10.)
                .tabular()
                .text_color(theme.content(0.45))
                .child(tag)
        });
        let full = format!("{}: {}", label.action, label.full);
        div()
            .id(eid(key, &format!("context:{index}")))
            .flex()
            .min_w_0()
            .max_w_full()
            .items_center()
            .gap(u(6.))
            .h(u(24.))
            .pl(u(6.))
            .pr(u(8.))
            .rounded(u(6.))
            .bg(theme.content(0.1))
            .text_px(11.)
            .line_height(u(11.))
            .text_color(theme.content(0.8))
            .tooltip(tooltip(full))
            .when_some(target, |el, (path, line)| {
                let cwd = self.cwd();
                el.cursor_pointer()
                    .hover(|s| s.text_color(theme.colors.content))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        let resolved = monocode_core::transcript::paths::resolve_workspace_path(
                            &path,
                            cwd.as_deref(),
                        )
                        .unwrap_or_else(|| path.clone());
                        cx.emit(TranscriptEvent::OpenFile {
                            path: resolved,
                            line: Some(line),
                        })
                    }))
            })
            .child(glyph)
            .child(
                div()
                    .min_w_0()
                    .max_w(u(224.))
                    .truncate()
                    .child(label.name.clone()),
            )
            .children(line_tag)
            .when_some(label.comment.clone(), |el, comment| {
                el.child(
                    div()
                        .min_w_0()
                        .max_w(u(192.))
                        .truncate()
                        .text_color(theme.content(0.55))
                        .child(comment),
                )
            })
            .into_any_element()
    }

    /// The compact link chip in a prompt (`UserLinkPreview compact`). A
    /// GitHub pull request or issue reads as "PR #73".
    fn render_link_chip(
        &mut self,
        key: &str,
        link: &UserLink,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let url = link.url.clone();
        let chip = div()
            .id(eid(key, "link"))
            .flex()
            .flex_none()
            .items_center()
            .mx(px(1.))
            .cursor_pointer()
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.emit(TranscriptEvent::OpenUrl { url: url.clone() })
            }));
        if let Some(item) = &link.github_work_item {
            let tone = if item.pull_request {
                palette::violet_400()
            } else {
                theme.colors.success
            };
            return chip
                .tooltip(tooltip(format!(
                    "Open {} #{} in {}",
                    if item.pull_request {
                        "pull request"
                    } else {
                        "issue"
                    },
                    item.number,
                    item.repo
                )))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(u(2.))
                        .h(u(18.))
                        .px(u(6.))
                        .rounded(u(6.))
                        .bg(with(tone, 0.1))
                        .text_xs_ui()
                        .medium()
                        .text_color(with(tone, 0.9))
                        .child(
                            icon(if item.pull_request {
                                IconName::GitPullRequest
                            } else {
                                IconName::CircleDot
                            })
                            .size(u(12.))
                            .text_color(with(tone, 0.9)),
                        )
                        .child(format!(
                            "{} #{}",
                            if item.pull_request { "PR" } else { "Issue" },
                            item.number
                        )),
                )
                .into_any_element();
        }
        chip.tooltip(tooltip(format!("Open {}", link.host)))
            .gap(u(4.))
            .rounded(u(6.))
            .px(u(6.))
            .bg(theme.content(0.06))
            .text_sm_ui()
            .text_color(theme.colors.link)
            .hover(|s| s.bg(theme.content(0.1)))
            .child(
                icon(IconName::Globe)
                    .size(u(12.))
                    .text_color(theme.colors.link),
            )
            .child(div().min_w_0().truncate().child(link.display_url.clone()))
            .into_any_element()
    }
}

/// `::highlight(monocode-transcript-search-match)` and `-current`: every
/// match of the search, with the first match of the current result in the
/// accent color.
fn search_highlighted(text: &str, query: &str, current: bool, theme: &Theme) -> gpui::StyledText {
    let shown = gpui::SharedString::from(text.to_string());
    let ranges = monocode_core::transcript::highlights::transcript_word_ranges([text], query)
        .into_iter()
        .next()
        .unwrap_or_default();
    let highlights = ranges
        .into_iter()
        .enumerate()
        .map(|(index, range)| {
            let color = if current && index == 0 {
                with(theme.colors.accent, 0.62)
            } else {
                with(monocode_ui::color::hex(0xe2c08d), 0.46)
            };
            (
                range,
                gpui::HighlightStyle {
                    background_color: Some(color),
                    ..Default::default()
                },
            )
        })
        .collect::<Vec<_>>();
    gpui::StyledText::new(shown).with_highlights(highlights)
}

fn with(color: Hsla, alpha: f32) -> Hsla {
    monocode_ui::color::with_alpha(color, alpha)
}

/// `AttachmentChip` as sent: an image thumbnail, or the file's icon and name.
fn attachment_chip(key: &str, index: usize, file: &Attachment, theme: &Theme) -> AnyElement {
    let path = file.path.clone().filter(|path| !path.is_empty());
    let title = path.clone().unwrap_or_else(|| file.name.clone());
    if file.kind == AttachmentKind::Image
        && let Some(path) = &path
    {
        return div()
            .id(eid(key, &format!("attachment:{index}")))
            .flex_none()
            .size(u(36.))
            .overflow_hidden()
            .rounded(u(8.))
            .tooltip(tooltip(title))
            .child(img(std::path::PathBuf::from(path)).size_full())
            .into_any_element();
    }
    let folder = file.mime_type == "inode/directory";
    div()
        .id(eid(key, &format!("attachment:{index}")))
        .flex()
        .min_w_0()
        .items_center()
        .gap(u(6.))
        .rounded(u(6.))
        .bg(theme.content(0.1))
        .py(u(2.))
        .px(u(4.))
        .tooltip(tooltip(title))
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(u(20.))
                .child(if folder {
                    folder_type_icon(file.name.clone(), false, false).into_any_element()
                } else {
                    file_type_icon(file.name.clone()).into_any_element()
                }),
        )
        .child(
            div()
                .min_w_0()
                .max_w(u(140.))
                .truncate()
                .text_px(11.)
                .line_height(u(11.))
                .text_color(theme.content(0.8))
                .child(file.name.clone()),
        )
        .into_any_element()
}
