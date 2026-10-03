//! Port of src/features/inbox/ui/CheckEvidence.tsx: a failed job's
//! annotations, each with a three-line source excerpt read at the checked
//! commit and a link to the same revision on GitHub. Reads are shared within
//! a job and keyed to its commit.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::LazyLock;

use gpui::{
    AnyElement, App, ClickEvent, ElementId, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _,
};
use monocode_core::js;
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, UiStyled as _, file_type_icon, icon, u};
use regex::Regex;

use crate::data::{GithubCheckAnnotation, InboxServices};
use crate::style::palette;

/// One annotated file read.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SourceLoad {
    Pending,
    Done(Option<String>),
}

/// The evidence state of one job row: "Show N more", and the file reads for
/// its commit.
#[derive(Debug, Default)]
pub struct EvidenceState {
    pub show_all: bool,
    head: String,
    sources: HashMap<String, SourceLoad>,
}

impl EvidenceState {
    /// The files to read now: shown annotations that can be read and have
    /// no read yet. Marks them pending. A new commit clears the reads.
    pub fn wanted(
        &mut self,
        cwd: &str,
        head_oid: &str,
        annotations: &[GithubCheckAnnotation],
    ) -> Vec<String> {
        let head = format!("{cwd}:{head_oid}");
        if head != self.head {
            self.head = head;
            self.sources.clear();
        }
        let shown = if self.show_all {
            annotations.len()
        } else {
            annotations.len().min(5)
        };
        let mut wanted = Vec::new();
        for annotation in &annotations[..shown] {
            let plan = AnnotationPlan::new(annotation, cwd, "", head_oid);
            if !plan.can_read || self.sources.contains_key(&plan.relative) {
                continue;
            }
            self.sources
                .insert(plan.relative.clone(), SourceLoad::Pending);
            wanted.push(plan.relative);
        }
        wanted
    }

    /// A read finished. Ignored when the commit moved on.
    pub fn finish(&mut self, head: &str, relative: String, text: Option<String>) {
        if head == self.head {
            self.sources.insert(relative, SourceLoad::Done(text));
        }
    }

    fn source(&self, relative: &str) -> Option<&SourceLoad> {
        self.sources.get(relative)
    }
}

static INVALID_PATH_CHARS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\\:\x00-\x1f]").expect("valid regex"));
static COMMIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:[a-f0-9]{40}|[a-f0-9]{64})$").expect("valid regex"));
static REPO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[\w.-]+/[\w.-]+$").expect("valid regex"));

/// What `CheckAnnotation` derives from an annotation before reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnotationPlan {
    pub relative: String,
    pub valid_line: bool,
    pub can_read: bool,
    pub file_url: Option<String>,
    pub location: String,
}

impl AnnotationPlan {
    pub fn new(annotation: &GithubCheckAnnotation, cwd: &str, repo: &str, head_oid: &str) -> Self {
        let relative = annotation
            .path
            .strip_prefix("./")
            .unwrap_or(&annotation.path)
            .to_string();
        let valid_path = !relative.is_empty()
            && !INVALID_PATH_CHARS.is_match(&relative)
            && relative
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != "..");
        let valid_commit = COMMIT.is_match(head_oid);
        let valid_line = annotation.line > 0;
        let can_read = !cwd.is_empty() && valid_path && valid_commit && valid_line;
        let valid_repo =
            REPO.is_match(repo) && repo.split('/').all(|part| part != "." && part != "..");
        let file_url = (valid_repo && valid_path && valid_commit).then(|| {
            let encoded = relative
                .split('/')
                .map(js::encode_uri_component)
                .collect::<Vec<_>>()
                .join("/");
            format!(
                "https://github.com/{repo}/blob/{head_oid}/{encoded}{}",
                if valid_line {
                    format!("#L{}", annotation.line)
                } else {
                    String::new()
                }
            )
        });
        let location = if valid_line {
            format!("{}:{}", annotation.path, annotation.line)
        } else {
            annotation.path.clone()
        };
        Self {
            relative,
            valid_line,
            can_read,
            file_url,
            location,
        }
    }
}

/// The lines around an annotation: the line before, the line, and the line
/// after, with the first line's number.
pub fn excerpt(text: &str, line: i64) -> (i64, Vec<String>) {
    let lines: Vec<&str> = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    let first = (line - 1).max(1);
    if line < 1 || line as usize > lines.len() {
        return (first, Vec::new());
    }
    let start = (first - 1) as usize;
    let end = (line as usize + 1).min(lines.len());
    (
        first,
        lines[start..end]
            .iter()
            .map(|line| line.to_string())
            .collect(),
    )
}

/// `CheckEvidence`.
#[allow(clippy::too_many_arguments)]
pub fn check_evidence(
    row_key: &str,
    annotations: &[GithubCheckAnnotation],
    state: &EvidenceState,
    cwd: &str,
    repo: &str,
    head_oid: &str,
    services: Rc<dyn InboxServices>,
    on_show_all: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let theme = Theme::of(cx);
    let shown = if state.show_all {
        annotations.len()
    } else {
        annotations.len().min(5)
    };
    let mut column = div().flex().flex_col().min_w_0().gap(u(12.));
    for (index, annotation) in annotations[..shown].iter().enumerate() {
        column = column.child(check_annotation(
            ElementId::Name(format!("{row_key}:annotation:{index}").into()),
            annotation,
            state,
            cwd,
            repo,
            head_oid,
            services.clone(),
            cx,
        ));
    }
    if !state.show_all && annotations.len() > 5 {
        let hover_bg = theme.content(0.05);
        let ink = theme.colors.content;
        column = column.child(
            div()
                .id(ElementId::Name(
                    format!("{row_key}:annotations-more").into(),
                ))
                .rounded(u(theme.radius.sm))
                .px(u(8.))
                .py(u(4.))
                .text_px(theme.text.caption)
                .text_color(theme.content(0.55))
                .hover(move |s| s.bg(hover_bg).text_color(ink))
                .on_click(on_show_all)
                .child(format!("Show {} more annotations", annotations.len() - 5)),
        );
    }
    column.into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn check_annotation(
    id: ElementId,
    annotation: &GithubCheckAnnotation,
    state: &EvidenceState,
    cwd: &str,
    repo: &str,
    head_oid: &str,
    services: Rc<dyn InboxServices>,
    cx: &App,
) -> AnyElement {
    let theme = Theme::of(cx);
    let plan = AnnotationPlan::new(annotation, cwd, repo, head_oid);
    let loaded = match state.source(&plan.relative) {
        Some(SourceLoad::Done(text)) if plan.can_read => Some(text.clone()),
        _ => None,
    };
    let (first_line, lines) = match loaded.as_ref() {
        Some(Some(text)) => excerpt(text, annotation.line),
        _ => (1, Vec::new()),
    };
    let mut message = annotation.message.split('\n');
    let title = message
        .next()
        .unwrap_or("")
        .trim_end_matches('\r')
        .to_string();
    let rest = message
        .map(|line| line.trim_end_matches('\r'))
        .collect::<Vec<_>>()
        .join("\n");
    let failure = annotation.level == "failure";
    let mut card = div()
        .min_w_0()
        .overflow_hidden()
        .rounded(u(theme.radius.lg))
        .border_1()
        .border_color(theme.colors.stroke)
        .bg(monocode_ui::color::with_alpha(
            theme.colors.background_base,
            0.35,
        ));
    if !annotation.path.is_empty() {
        let mut header = div()
            .flex()
            .min_w_0()
            .items_center()
            .gap(u(6.))
            .border_b_1()
            .border_color(theme.colors.stroke)
            .bg(theme.content(0.02))
            .px(u(12.))
            .py(u(8.))
            .text_px(theme.text.label)
            .text_color(theme.content(0.65))
            .child(file_type_icon(SharedString::from(annotation.path.clone())).size(13.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(plan.location.clone()),
            );
        if let Some(url) = plan.file_url.clone() {
            let hover_bg = theme.content(0.05);
            let ink = theme.colors.content;
            header = header.child(
                div()
                    .id(id.clone())
                    .group("annotation-link")
                    .flex()
                    .flex_none()
                    .size(u(24.))
                    .my(u(-4.))
                    .mr(u(-4.))
                    .items_center()
                    .justify_center()
                    .rounded(u(theme.radius.sm))
                    .hover(move |s| s.bg(hover_bg))
                    .tooltip(tooltip("View source at the checked commit"))
                    .on_click(move |_, _, cx| services.open_url(&url, cx))
                    .child(
                        icon(IconName::ExternalLink)
                            .size(u(12.))
                            .text_color(theme.content(0.40))
                            .group_hover("annotation-link", move |s| s.text_color(ink)),
                    ),
            );
        }
        card = card.child(header);
    }
    if !lines.is_empty() {
        let mut code = div()
            .py(u(8.))
            .font_family(theme.fonts.mono.clone())
            .text_px(theme.text.caption)
            .line_height(u(20.));
        for (offset, line) in lines.iter().enumerate() {
            let number = first_line + offset as i64;
            let current = number == annotation.line;
            let rose = palette::rose_400();
            code = code.child(
                div()
                    .flex()
                    .gap(u(16.))
                    .border_l_2()
                    .pr(u(12.))
                    .border_color(if current {
                        monocode_ui::color::with_alpha(rose, 0.4)
                    } else {
                        gpui::transparent_black()
                    })
                    .when(current, |row| {
                        row.bg(monocode_ui::color::with_alpha(rose, 0.06))
                    })
                    .text_color(if current {
                        theme.content(0.85)
                    } else {
                        theme.content(0.45)
                    })
                    .child(
                        div()
                            .flex_none()
                            .w(u(32.))
                            .text_right()
                            .tabular()
                            .text_color(theme.content(0.30))
                            .child(number.to_string()),
                    )
                    .child(div().whitespace_nowrap().child(if line.is_empty() {
                        " ".to_string()
                    } else {
                        line.clone()
                    })),
            );
        }
        card = card.child(code);
    }
    let mark_ink = if failure {
        monocode_ui::color::with_alpha(palette::rose_400(), 0.7)
    } else {
        monocode_ui::color::with_alpha(theme.colors.warning, 0.7)
    };
    let unavailable = plan.can_read && loaded.is_some() && lines.is_empty();
    card = card.child(
        div()
            .flex()
            .min_w_0()
            .items_start()
            .gap(u(8.))
            .px(u(12.))
            .py(u(12.))
            .when(!lines.is_empty(), |row| {
                row.border_t_1().border_color(theme.colors.stroke)
            })
            .child(
                icon(if failure {
                    IconName::CircleX
                } else {
                    IconName::AlertCircle
                })
                .mt(u(2.))
                .size(u(12.))
                .text_color(mark_ink),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_px(theme.text.label)
                            .text_color(theme.content(0.75))
                            .child(title),
                    )
                    .when(!rest.is_empty(), |column| {
                        column.child(
                            div()
                                .mt(u(6.))
                                .font_family(theme.fonts.mono.clone())
                                .text_px(theme.text.caption)
                                .leading(theme.leading.relaxed)
                                .text_color(theme.content(0.65))
                                .child(rest.clone()),
                        )
                    })
                    .when(unavailable, |column| {
                        column.child(
                            div()
                                .mt(u(8.))
                                .text_px(theme.text.micro)
                                .text_color(theme.content(0.40))
                                .child("Source preview unavailable for this commit."),
                        )
                    }),
            ),
    );
    card.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn annotation(path: &str, line: i64) -> GithubCheckAnnotation {
        GithubCheckAnnotation {
            path: path.into(),
            line,
            message: "Assertion failed".into(),
            level: "failure".into(),
        }
    }

    #[test]
    fn links_annotations_to_the_checked_revision() {
        let head = "a".repeat(40);
        let plan = AnnotationPlan::new(
            &annotation("src/preview test.ts", 2),
            "/tmp/web",
            "acme/web",
            &head,
        );
        assert!(plan.can_read);
        assert_eq!(plan.location, "src/preview test.ts:2");
        assert_eq!(
            plan.file_url.as_deref(),
            Some(
                format!("https://github.com/acme/web/blob/{head}/src/preview%20test.ts#L2")
                    .as_str()
            )
        );
    }

    #[test]
    fn refuses_paths_and_commits_it_cannot_read() {
        let plan = AnnotationPlan::new(&annotation("../etc/passwd", 2), "/tmp", "acme/web", "abc");
        assert!(!plan.can_read);
        assert!(plan.file_url.is_none());
    }

    #[test]
    fn excerpts_the_line_and_its_neighbours() {
        let text = "const status = response.status;\r\nexpect(status).toBe(200);\nfinish();";
        let (first, lines) = excerpt(text, 2);
        assert_eq!(first, 1);
        assert_eq!(
            lines,
            [
                "const status = response.status;",
                "expect(status).toBe(200);",
                "finish();"
            ]
        );
        assert!(excerpt(text, 9).1.is_empty());
    }
}
