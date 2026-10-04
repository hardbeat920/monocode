//! Port of src/features/sessions/ui/UserLinkPreview.tsx: the web link in a
//! prompt. A GitHub pull request or issue reads as a compact "PR #73" chip;
//! hovering it for 220ms (or focusing it) opens a card with the item's
//! state, title, summary, author, branches, labels, and assignees. Other
//! links show the page's favicon and title.
//!
//! The details come from [`LinkPreviews`], a cache the host fills, the way
//! the React code read `peekGithubWorkItem` and `fetchLinkPreviewMetadata`.
//! The host installs a loader with [`LinkPreviews::set_loader`] and answers
//! each [`LinkPreviewRequest`] with [`LinkPreviews::resolve_work_item`] or
//! [`LinkPreviews::resolve_metadata`]. A click reports
//! [`UserLinkPreviewEvent::Open`].

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Anchor, Animation, AnimationExt as _, AnyElement, App, Context, ElementId, EventEmitter,
    FocusHandle, Focusable, Global, Hsla, Image, ImageFormat, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    StyledImage as _, Subscription, Task, Window, div, img, point, px, relative,
};
use monocode_ui::color::{parse_hex, with_alpha};
use monocode_ui::styled::UiStyled as _;
use monocode_ui::widgets::{PopoverSide, popover_at, popover_frame};
use monocode_ui::{IconName, Theme, icon, u};
use regex::Regex;

use crate::transcript::model::link::{GithubWorkItem as ParsedWorkItem, UserLink};

use super::style;
use super::util::BoundsMap;

/// `HOVER_OPEN_DELAY_MS`.
pub const HOVER_OPEN_DELAY: Duration = Duration::from_millis(220);
/// `HOVER_CLOSE_DELAY_MS`.
pub const HOVER_CLOSE_DELAY: Duration = Duration::from_millis(100);

/// A label on a work item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkItemLabel {
    pub name: String,
    /// Six hex digits without `#`, as GitHub sends them.
    pub color: String,
}

/// An assignee or author.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkItemPerson {
    pub login: String,
    pub avatar_url: Option<String>,
}

/// The parts of `GithubWorkItem` the card shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkWorkItem {
    pub title: String,
    /// `open`, `closed`, or `merged`.
    pub state: String,
    pub draft: bool,
    /// `formatRelativeTime(updatedAt)`, such as "2 hours ago".
    pub updated: Option<String>,
    pub labels: Vec<WorkItemLabel>,
    pub assignees: Vec<WorkItemPerson>,
}

/// The parts of `GithubWorkItemDetails` the card shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LinkWorkItemDetails {
    pub body: String,
    pub author: String,
    pub author_avatar_url: Option<String>,
    pub base_ref_name: Option<String>,
    pub head_ref_name: Option<String>,
}

/// `LinkPreviewMetadata`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LinkPreviewMetadata {
    pub title: Option<String>,
    /// A `data:image/...;base64,` URL.
    pub favicon_data_url: Option<String>,
}

/// What the cache needs the host to fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkPreviewRequest {
    /// `fetchLinkPreviewMetadata(url)`.
    Metadata { url: String },
    /// `githubWorkItem` and `githubWorkItemDetails` for one item.
    WorkItem {
        cwd: String,
        repo: String,
        pull_request: bool,
        number: i64,
    },
}

/// `loadState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadState {
    Idle,
    Loading,
    Ready,
    Unavailable,
}

/// The cache key of a work item: `repo:kind:number`.
pub fn work_item_key(item: &ParsedWorkItem) -> String {
    format!(
        "{}:{}:{}",
        item.repo,
        if item.pull_request { "pr" } else { "issue" },
        item.number
    )
}

type Loader = Rc<dyn Fn(LinkPreviewRequest, &mut App)>;

/// The link preview cache: page metadata by URL and work items by key.
#[derive(Default)]
pub struct LinkPreviews {
    metadata: HashMap<String, LinkPreviewMetadata>,
    items: HashMap<String, LinkWorkItem>,
    details: HashMap<String, LinkWorkItemDetails>,
    /// Requests that finished without anything to show.
    failed: HashSet<String>,
    in_flight: HashSet<String>,
    loader: Option<Loader>,
}

impl Global for LinkPreviews {}

impl LinkPreviews {
    /// Install the fetcher. Requests made before it was set are dropped.
    pub fn set_loader(cx: &mut App, loader: impl Fn(LinkPreviewRequest, &mut App) + 'static) {
        cx.default_global::<LinkPreviews>().loader = Some(Rc::new(loader));
    }

    pub fn metadata(url: &str, cx: &App) -> Option<LinkPreviewMetadata> {
        cx.try_global::<LinkPreviews>()
            .and_then(|cache| cache.metadata.get(url).cloned())
    }

    /// `peekGithubWorkItem`.
    pub fn work_item(key: &str, cx: &App) -> Option<LinkWorkItem> {
        cx.try_global::<LinkPreviews>()
            .and_then(|cache| cache.items.get(key).cloned())
    }

    /// `peekGithubWorkItemDetails`.
    pub fn work_item_details(key: &str, cx: &App) -> Option<LinkWorkItemDetails> {
        cx.try_global::<LinkPreviews>()
            .and_then(|cache| cache.details.get(key).cloned())
    }

    /// Whether a work item request ended with neither part.
    pub fn work_item_failed(key: &str, cx: &App) -> bool {
        cx.try_global::<LinkPreviews>()
            .is_some_and(|cache| cache.failed.contains(key))
    }

    /// Ask the host for something once. Returns whether a request went out.
    pub fn request(request: LinkPreviewRequest, cx: &mut App) -> bool {
        let key = match &request {
            LinkPreviewRequest::Metadata { url } => format!("url:{url}"),
            LinkPreviewRequest::WorkItem {
                repo,
                pull_request,
                number,
                ..
            } => format!(
                "item:{repo}:{}:{number}",
                if *pull_request { "pr" } else { "issue" }
            ),
        };
        let cache = cx.default_global::<LinkPreviews>();
        let Some(loader) = cache.loader.clone() else {
            return false;
        };
        if !cache.in_flight.insert(key) {
            return false;
        }
        loader(request, cx);
        true
    }

    /// The page metadata for `url` arrived. An error keeps the host name.
    pub fn resolve_metadata(url: &str, result: Result<LinkPreviewMetadata, String>, cx: &mut App) {
        let cache = cx.default_global::<LinkPreviews>();
        cache.in_flight.remove(&format!("url:{url}"));
        if let Ok(metadata) = result {
            cache.metadata.insert(url.to_string(), metadata);
        }
    }

    /// `Promise.allSettled([githubWorkItem, githubWorkItemDetails])`.
    pub fn resolve_work_item(
        key: &str,
        item: Result<LinkWorkItem, String>,
        details: Result<LinkWorkItemDetails, String>,
        cx: &mut App,
    ) {
        let cache = cx.default_global::<LinkPreviews>();
        cache.in_flight.remove(&format!("item:{key}"));
        let any = item.is_ok() || details.is_ok();
        if let Ok(item) = item {
            cache.items.insert(key.to_string(), item);
        }
        if let Ok(details) = details {
            cache.details.insert(key.to_string(), details);
        }
        if any {
            cache.failed.remove(key);
        } else {
            cache.failed.insert(key.to_string());
        }
    }
}

static IMAGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"!\[[^\]]*\]\([^)]*\)").expect("image"));
static LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\]]+)\]\([^)]*\)").expect("link"));
static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").expect("tag"));
static MARKUP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[`*_>#~|-]+").expect("markup"));
static SPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("space"));

/// `plainTextSummary`: a body's text without Markdown or HTML.
pub fn plain_text_summary(value: &str) -> String {
    let value = IMAGE.replace_all(value, "");
    let value = LINK.replace_all(&value, "$1");
    let value = TAG.replace_all(&value, " ");
    let value = MARKUP.replace_all(&value, " ");
    let value = SPACE.replace_all(&value, " ");
    monocode_core::js::trim(&value).to_string()
}

/// `labelColor`: a GitHub label's dot, or `None` for the text color.
pub fn label_color(value: &str) -> Option<Hsla> {
    let value = monocode_core::js::trim(value);
    if value.len() == 6 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        parse_hex(&format!("#{value}"))
    } else {
        None
    }
}

/// How a work item's state reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusTone {
    Muted,
    Open,
    Merged,
    Closed,
}

/// `workItemStatus`: the state's icon, label, and color.
pub fn work_item_status(
    pull_request: bool,
    item: Option<&LinkWorkItem>,
) -> (IconName, &'static str, StatusTone) {
    let Some(item) = item else {
        return if pull_request {
            (IconName::GitPullRequest, "Pull request", StatusTone::Muted)
        } else {
            (IconName::CircleDot, "Issue", StatusTone::Muted)
        };
    };
    if item.draft {
        return (IconName::GitPullRequestDraft, "Draft", StatusTone::Muted);
    }
    if item.state == "merged" {
        return (IconName::GitMerge, "Merged", StatusTone::Merged);
    }
    if item.state == "closed" {
        return (
            if pull_request {
                IconName::GitPullRequestClosed
            } else {
                IconName::CircleX
            },
            "Closed",
            StatusTone::Closed,
        );
    }
    (
        if pull_request {
            IconName::GitPullRequest
        } else {
            IconName::CircleDot
        },
        "Open",
        StatusTone::Open,
    )
}

fn tone_color(tone: StatusTone, theme: &Theme) -> Hsla {
    match tone {
        StatusTone::Muted => theme.content(0.5),
        StatusTone::Open => style::emerald_400(0.9),
        StatusTone::Merged => style::violet_400(0.9),
        StatusTone::Closed => style::rose(0.9),
    }
}

/// The chip's text: "PR #73" or "Issue #42".
pub fn chip_text(item: &ParsedWorkItem) -> String {
    format!(
        "{} #{}",
        if item.pull_request { "PR" } else { "Issue" },
        item.number
    )
}

/// The chip's accessible name.
pub fn chip_label(item: &ParsedWorkItem, title: Option<&str>) -> String {
    format!(
        "Open {} #{} in {}{}",
        if item.pull_request {
            "pull request"
        } else {
            "issue"
        },
        item.number,
        item.repo,
        title
            .filter(|title| !title.is_empty())
            .map(|title| format!(": {title}"))
            .unwrap_or_default()
    )
}

/// The branch line of a pull request: `main ← feature`.
pub fn branch_line(details: &LinkWorkItemDetails) -> Option<String> {
    let base = details
        .base_ref_name
        .as_deref()
        .filter(|name| !name.is_empty())?;
    let head = details
        .head_ref_name
        .as_deref()
        .filter(|name| !name.is_empty())?;
    Some(format!("{base} \u{2190} {head}"))
}

/// A favicon from a `data:` URL.
pub fn favicon_image(data_url: &str) -> Option<Arc<Image>> {
    use base64::Engine as _;
    let rest = data_url.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    if !meta.ends_with(";base64") {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .ok()?;
    let mime = super::generated_image::sniff_image_mime(&bytes);
    let format = match (mime, meta) {
        (Some("image/png"), _) => ImageFormat::Png,
        (Some("image/jpeg"), _) => ImageFormat::Jpeg,
        (Some("image/gif"), _) => ImageFormat::Gif,
        (Some("image/webp"), _) => ImageFormat::Webp,
        (Some("image/x-icon"), _) => ImageFormat::Ico,
        (Some("image/bmp"), _) => ImageFormat::Bmp,
        (None, meta) if meta.starts_with("image/svg+xml") => ImageFormat::Svg,
        _ => return None,
    };
    Some(Arc::new(Image::from_bytes(format, bytes)))
}

/// What the reader did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserLinkPreviewEvent {
    /// Open the original URL in the browser.
    Open { url: String },
}

/// `<UserLinkPreview link cwd compact />`.
pub struct UserLinkPreview {
    link: UserLink,
    cwd: Option<String>,
    compact: bool,
    open: bool,
    request_started: bool,
    load_state: LoadState,
    focus: FocusHandle,
    bounds: BoundsMap,
    open_timer: Option<Task<()>>,
    close_timer: Option<Task<()>>,
    _cache: Subscription,
    _focus: Vec<Subscription>,
}

impl EventEmitter<UserLinkPreviewEvent> for UserLinkPreview {}

impl UserLinkPreview {
    pub fn new(
        link: UserLink,
        cwd: Option<String>,
        compact: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let cache = cx.observe_global::<LinkPreviews>(|this: &mut Self, cx| {
            this.sync_load_state(cx);
            cx.notify();
        });
        let focus = cx.focus_handle();
        let subscriptions = vec![
            cx.on_focus(&focus, window, |this, _, cx| this.show_now(cx)),
            cx.on_blur(&focus, window, |this, _, cx| this.hide_now(cx)),
        ];
        let mut this = Self {
            link,
            cwd,
            compact,
            open: false,
            request_started: false,
            load_state: LoadState::Idle,
            focus,
            bounds: BoundsMap::default(),
            open_timer: None,
            close_timer: None,
            _cache: cache,
            _focus: subscriptions,
        };
        this.sync_load_state(cx);
        if this.link.github_work_item.is_none() {
            LinkPreviews::request(
                LinkPreviewRequest::Metadata {
                    url: this.link.url.clone(),
                },
                cx,
            );
        }
        this
    }

    /// Show another link. A different work item starts over.
    pub fn set_link(
        &mut self,
        link: UserLink,
        cwd: Option<String>,
        compact: bool,
        cx: &mut Context<Self>,
    ) {
        if link == self.link && cwd == self.cwd && compact == self.compact {
            return;
        }
        if link != self.link {
            self.open = false;
            self.request_started = false;
            self.load_state = LoadState::Idle;
            if link.github_work_item.is_none() {
                LinkPreviews::request(
                    LinkPreviewRequest::Metadata {
                        url: link.url.clone(),
                    },
                    cx,
                );
            }
        }
        self.link = link;
        self.cwd = cwd;
        self.compact = compact;
        self.sync_load_state(cx);
        cx.notify();
    }

    pub fn link(&self) -> &UserLink {
        &self.link
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn load_state(&self) -> LoadState {
        self.load_state
    }

    fn key(&self) -> Option<String> {
        self.link.github_work_item.as_ref().map(work_item_key)
    }

    /// The cached item and details, if any.
    pub fn work_item(&self, cx: &App) -> (Option<LinkWorkItem>, Option<LinkWorkItemDetails>) {
        match self.key() {
            Some(key) => (
                LinkPreviews::work_item(&key, cx),
                LinkPreviews::work_item_details(&key, cx),
            ),
            None => (None, None),
        }
    }

    fn sync_load_state(&mut self, cx: &App) {
        let Some(key) = self.key() else {
            return;
        };
        let (item, details) = self.work_item(cx);
        if item.is_some() || details.is_some() {
            self.load_state = LoadState::Ready;
        } else if LinkPreviews::work_item_failed(&key, cx) && self.request_started {
            self.load_state = LoadState::Unavailable;
        }
    }

    /// `load`: fetch the item and its details once.
    fn load(&mut self, cx: &mut Context<Self>) {
        if self.request_started {
            return;
        }
        let Some(parsed) = self.link.github_work_item.clone() else {
            return;
        };
        self.request_started = true;
        let (item, details) = self.work_item(cx);
        if item.is_some() && details.is_some() {
            return;
        }
        if item.is_none() || details.is_none() {
            self.load_state = if item.is_some() || details.is_some() {
                LoadState::Ready
            } else {
                LoadState::Loading
            };
        }
        let cwd = self
            .cwd
            .clone()
            .filter(|cwd| !cwd.is_empty())
            .unwrap_or_else(|| ".".into());
        let sent = LinkPreviews::request(
            LinkPreviewRequest::WorkItem {
                cwd,
                repo: parsed.repo,
                pull_request: parsed.pull_request,
                number: parsed.number,
            },
            cx,
        );
        if !sent
            && cx
                .try_global::<LinkPreviews>()
                .is_none_or(|cache| cache.loader.is_none())
        {
            // No host to ask: say so instead of loading forever.
            self.load_state = LoadState::Unavailable;
        }
    }

    /// `showNow`: focus opens the card at once.
    pub fn show_now(&mut self, cx: &mut Context<Self>) {
        self.open_timer = None;
        self.close_timer = None;
        self.open = true;
        self.load(cx);
        cx.notify();
    }

    /// `showAfterDelay`: the pointer entered the chip.
    pub fn show_after_delay(&mut self, cx: &mut Context<Self>) {
        self.close_timer = None;
        if self.open || self.open_timer.is_some() {
            return;
        }
        self.open_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(HOVER_OPEN_DELAY).await;
            this.update(cx, |this, cx| {
                this.open_timer = None;
                this.open = true;
                this.load(cx);
                cx.notify();
            })
            .ok();
        }));
    }

    /// `hideAfterDelay`: the pointer left the chip or the card.
    pub fn hide_after_delay(&mut self, cx: &mut Context<Self>) {
        self.open_timer = None;
        self.close_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(HOVER_CLOSE_DELAY).await;
            this.update(cx, |this, cx| {
                this.close_timer = None;
                this.open = false;
                cx.notify();
            })
            .ok();
        }));
    }

    /// `clearCloseTimer`: the pointer entered the card.
    pub fn keep_open(&mut self) {
        self.close_timer = None;
    }

    /// `hideNow`.
    pub fn hide_now(&mut self, cx: &mut Context<Self>) {
        self.open_timer = None;
        self.close_timer = None;
        self.open = false;
        cx.notify();
    }

    /// A click opens the original URL.
    pub fn click(&mut self, cx: &mut Context<Self>) {
        self.hide_now(cx);
        cx.emit(UserLinkPreviewEvent::Open {
            url: self.link.url.clone(),
        });
    }

    fn render_generic(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let metadata = LinkPreviews::metadata(&self.link.url, cx);
        let title = metadata
            .as_ref()
            .and_then(|metadata| metadata.title.as_deref())
            .map(monocode_core::js::trim)
            .filter(|title| !title.is_empty())
            .unwrap_or(&self.link.host)
            .to_string();
        let favicon = metadata
            .as_ref()
            .and_then(|metadata| metadata.favicon_data_url.as_deref())
            .and_then(favicon_image);
        let letter: SharedString = self
            .link
            .host
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default()
            .into();
        let letter_box = {
            let letter = letter.clone();
            let theme = theme.clone();
            move || {
                div()
                    .text_px(9.)
                    .semibold()
                    .text_color(theme.content(0.55))
                    .child(letter.clone())
                    .into_any_element()
            }
        };
        let mark = div()
            .mr(u(4.))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(u(16.))
            .overflow_hidden()
            .rounded(u(4.))
            .bg(with_alpha(theme.colors.background_base, 0.5))
            .child(match favicon {
                Some(image) => img(image)
                    .size(u(14.))
                    .object_fit(gpui::ObjectFit::Contain)
                    .with_fallback(letter_box)
                    .into_any_element(),
                None => letter_box(),
            });
        let link = style::sky_400(0.9);
        div()
            .id("user-link-preview")
            .flex()
            .flex_none()
            .items_center()
            .mx(u(2.))
            .text_color(link)
            .cursor_pointer()
            .hover(|s| s.text_color(style::sky_300()).underline())
            .tooltip(monocode_ui::widgets::tooltip(self.link.url.clone()))
            .on_click(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.click(cx);
            }))
            .child(mark)
            .child(div().min_w_0().truncate().medium().child(title))
            .into_any_element()
    }

    fn render_github(
        &self,
        parsed: &ParsedWorkItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let compact = self.compact;
        let focused = self.focus.is_focused(window);
        let tone = if parsed.pull_request {
            |alpha: f32| style::violet_400(alpha)
        } else {
            |alpha: f32| style::emerald_400(alpha)
        };
        let pill = div()
            .flex()
            .flex_none()
            .items_center()
            .medium()
            .px(u(6.))
            .bg(tone(0.1))
            .text_color(tone(0.9))
            .map(|el| {
                if compact {
                    el.h(u(18.)).gap(u(2.)).rounded(u(6.))
                } else {
                    el.h(u(20.)).gap(u(4.)).rounded(u(5.))
                }
            })
            .child(
                icon(if parsed.pull_request {
                    IconName::GitPullRequest
                } else {
                    IconName::CircleDot
                })
                .size(u(if compact { 12. } else { 14. }))
                .text_color(tone(0.9)),
            )
            .child(chip_text(parsed));
        let chip = div()
            .id("github-work-item-chip")
            .relative()
            .track_focus(&self.focus)
            .flex()
            .flex_none()
            .max_w_full()
            .items_center()
            .gap(u(4.))
            .mx(px(1.))
            .pb(px(1.))
            .rounded(u(6.))
            .text_px(if compact { 12. } else { 11. })
            .line_height(u(16.))
            .text_color(theme.content(0.7))
            .cursor_pointer()
            .hover(|s| s.text_color(theme.colors.content))
            .when(focused, |el| {
                el.shadow(vec![gpui::BoxShadow {
                    color: theme.accent(0.6),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(0.),
                    spread_radius: px(2.),
                    inset: false,
                }])
            })
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if *hovered {
                    this.show_after_delay(cx);
                } else {
                    this.hide_after_delay(cx);
                }
            }))
            // A click opens the link without focusing the chip, as in WebKit.
            .capture_any_mouse_down(|_, window, _| window.prevent_default())
            .on_click(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.click(cx);
            }))
            .child(self.bounds.track("chip"))
            .child(pill);
        let popover = (self.open)
            .then(|| self.bounds.get("chip"))
            .flatten()
            .map(|anchor| {
                let (item, details) = self.work_item(cx);
                let card = github_work_item_card(
                    parsed,
                    item.as_ref(),
                    details.as_ref(),
                    self.load_state,
                    &theme,
                );
                let frame = popover_frame("github-work-item-popover")
                    .side(PopoverSide::Top)
                    .width(360.)
                    .animate(!cx.reduce_motion())
                    .child(
                        div()
                            .id("github-work-item-card")
                            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                                if *hovered {
                                    this.keep_open();
                                } else {
                                    this.hide_after_delay(cx);
                                }
                            }))
                            .p(u(14.))
                            .font_family(theme.fonts.sans.clone())
                            .text_color(theme.colors.content)
                            .child(card),
                    );
                popover_at(
                    point(anchor.left(), anchor.top() - px(6.)),
                    Anchor::BottomLeft,
                    frame,
                    cx,
                )
            });
        div()
            .flex_none()
            .child(chip)
            .when_some(popover, |el, popover| el.child(popover))
            .into_any_element()
    }
}

impl Focusable for UserLinkPreview {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for UserLinkPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let body = match self.link.github_work_item.clone() {
            Some(parsed) => self.render_github(&parsed, window, cx),
            None => self.render_generic(cx),
        };
        div()
            .flex()
            .flex_none()
            .font_family(theme.fonts.sans.clone())
            .child(body)
    }
}

/// `GithubWorkItemCard`.
pub fn github_work_item_card(
    parsed: &ParsedWorkItem,
    item: Option<&LinkWorkItem>,
    details: Option<&LinkWorkItemDetails>,
    load_state: LoadState,
    theme: &Theme,
) -> AnyElement {
    let (status_icon, status_label, tone) = work_item_status(parsed.pull_request, item);
    let status_color = tone_color(tone, theme);
    let summary = plain_text_summary(details.map(|details| details.body.as_str()).unwrap_or(""));
    let updated = item
        .and_then(|item| item.updated.clone())
        .filter(|text| !text.is_empty());
    let labels = item.map(|item| item.labels.clone()).unwrap_or_default();
    let assignees = item.map(|item| item.assignees.clone()).unwrap_or_default();
    let branches = details
        .filter(|_| parsed.pull_request)
        .and_then(branch_line);
    let author = details
        .map(|details| (details.author.clone(), details.author_avatar_url.clone()))
        .filter(|(author, _)| !author.is_empty());

    let summary_el = |summary: String| {
        div()
            .mt(u(6.))
            .line_clamp(3)
            .text_px(11.)
            .leading(1.45)
            .text_color(theme.content(0.55))
            .child(summary)
    };
    let header = div()
        .flex()
        .min_w_0()
        .items_center()
        .gap(u(8.))
        .text_px(11.)
        .child(
            icon(status_icon)
                .size(u(16.))
                .flex_none()
                .text_color(status_color),
        )
        .child(
            div()
                .min_w_0()
                .truncate()
                .medium()
                .text_color(theme.content(0.65))
                .child(parsed.repo.clone()),
        )
        .child(
            div()
                .flex_none()
                .text_color(theme.content(0.35))
                .child("\u{b7}"),
        )
        .child(
            div()
                .flex_none()
                .font_family(theme.fonts.mono.clone())
                .tabular()
                .text_color(theme.content(0.5))
                .child(format!("#{}", parsed.number)),
        )
        .child(
            div()
                .ml_auto()
                .flex_none()
                .rounded_full()
                .bg(theme.content(0.07))
                .px(u(8.))
                .py(u(2.))
                .medium()
                .text_color(status_color)
                .child(status_label),
        );

    let body: AnyElement = if let Some(item) = item {
        div()
            .child(
                div()
                    .mt(u(8.))
                    .line_clamp(2)
                    .text_px(13.)
                    .semibold()
                    .leading(1.35)
                    .text_color(theme.colors.content)
                    .child(item.title.clone()),
            )
            .when(!summary.is_empty(), |el| {
                el.child(summary_el(summary.clone()))
            })
            .into_any_element()
    } else if matches!(load_state, LoadState::Idle | LoadState::Loading) {
        skeleton(theme)
    } else {
        div()
            .mt(u(8.))
            .child(
                div()
                    .text_px(13.)
                    .semibold()
                    .text_color(theme.colors.content)
                    .child(format!(
                        "{} #{}",
                        if parsed.pull_request {
                            "Pull request"
                        } else {
                            "Issue"
                        },
                        parsed.number
                    )),
            )
            .map(|el| {
                if summary.is_empty() {
                    el.child(
                        div()
                            .mt(u(4.))
                            .text_px(11.)
                            .leading(1.625)
                            .text_color(theme.content(0.5))
                            .child(
                                "Details aren\u{2019}t available here, but the link can still be opened on GitHub.",
                            ),
                    )
                } else {
                    el.child(summary_el(summary.clone()))
                }
            })
            .into_any_element()
    };

    let meta = (author.is_some() || updated.is_some()).then(|| {
        div()
            .mt(u(10.))
            .flex()
            .min_w_0()
            .items_center()
            .gap(u(6.))
            .text_px(10.)
            .text_color(theme.content(0.45))
            .when_some(author.clone(), |el, (name, avatar)| {
                el.child(
                    div()
                        .flex()
                        .min_w_0()
                        .items_center()
                        .gap(u(6.))
                        .child(avatar_element(&name, avatar.as_deref(), 16., theme))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .medium()
                                .text_color(theme.content(0.55))
                                .child(name),
                        ),
                )
            })
            .when(author.is_some() && updated.is_some(), |el| {
                el.child("\u{b7}")
            })
            .when_some(updated.clone(), |el, updated| {
                el.child(div().flex_none().child(format!("Updated {updated}")))
            })
    });

    let branch_el = branches.map(|line| {
        div()
            .mt(u(8.))
            .flex()
            .min_w_0()
            .items_center()
            .gap(u(6.))
            .rounded(u(6.))
            .bg(theme.content(0.045))
            .px(u(8.))
            .py(u(6.))
            .text_px(10.)
            .text_color(theme.content(0.5))
            .child(
                icon(IconName::GitCompare)
                    .size(u(14.))
                    .flex_none()
                    .text_color(theme.content(0.5)),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_family(theme.fonts.mono.clone())
                    .child(line),
            )
    });

    let tags = (!labels.is_empty() || !assignees.is_empty()).then(|| {
        let mut chips = div()
            .flex()
            .min_w_0()
            .flex_1()
            .flex_wrap()
            .items_center()
            .gap(u(4.));
        for label in labels.iter().take(3) {
            let dot = label_color(&label.color).unwrap_or(theme.content(0.55));
            chips = chips.child(
                div()
                    .flex()
                    .max_w(u(112.))
                    .items_center()
                    .gap(u(4.))
                    .rounded_full()
                    .border_1()
                    .border_color(theme.content(0.1))
                    .bg(theme.content(0.035))
                    .px(u(6.))
                    .py(u(2.))
                    .text_px(9.)
                    .text_color(theme.content(0.55))
                    .child(div().size(u(6.)).flex_none().rounded_full().bg(dot))
                    .child(div().min_w_0().truncate().child(label.name.clone())),
            );
        }
        if labels.len() > 3 {
            chips = chips.child(
                div()
                    .px(u(4.))
                    .text_px(9.)
                    .text_color(theme.content(0.35))
                    .child(format!("+{}", labels.len() - 3)),
            );
        }
        let people = (!assignees.is_empty()).then(|| {
            let mut row = div().flex().flex_none();
            for (index, person) in assignees.iter().take(3).enumerate() {
                row = row.child(
                    div()
                        .when(index > 0, |el| el.relative().left(u(-4. * index as f32)))
                        .child(avatar_element(
                            &person.login,
                            person.avatar_url.as_deref(),
                            18.,
                            theme,
                        )),
                );
            }
            row
        });
        div()
            .mt(u(10.))
            .flex()
            .min_w_0()
            .items_center()
            .gap(u(6.))
            .child(chips)
            .children(people)
    });

    let footer = div()
        .mt(u(12.))
        .flex()
        .items_center()
        .gap(u(6.))
        .border_t(px(1.))
        .border_color(theme.content(0.07))
        .pt(u(8.))
        .text_px(10.)
        .text_color(theme.content(0.35))
        .child(
            icon(IconName::ExternalLink)
                .size(u(12.))
                .text_color(theme.content(0.35)),
        )
        .child("Click the chip to open on GitHub");

    div()
        .min_w_0()
        .child(header)
        .child(body)
        .children(meta)
        .children(branch_el)
        .children(tags)
        .child(footer)
        .into_any_element()
}

/// `GithubWorkItemCardSkeleton`: three pulsing bars.
fn skeleton(theme: &Theme) -> AnyElement {
    let bar = |id: &'static str, height: f32, width: f32, alpha: f32| {
        div()
            .h(u(height))
            .w(relative(width))
            .rounded(u(4.))
            .bg(theme.content(alpha))
            .with_animation(
                id,
                Animation::new(Duration::from_secs(2))
                    .repeat()
                    .with_easing(monocode_ui::theme::CubicBezier(0.4, 0., 0.6, 1.).easing()),
                |el, t| el.opacity(1. - 0.5 * (1. - (t * 2. - 1.).abs())),
            )
    };
    div()
        .mt(u(10.))
        .flex()
        .flex_col()
        .gap(u(8.))
        .child(bar("skeleton-title", 12., 0.8, 0.1))
        .child(bar("skeleton-line-1", 8., 1., 0.07))
        .child(bar("skeleton-line-2", 8., 2. / 3., 0.07))
        .into_any_element()
}

/// `GithubAvatar`: the picture, or the first letter when there is none or
/// it fails to load.
fn avatar_element(name: &str, avatar_url: Option<&str>, size: f32, theme: &Theme) -> AnyElement {
    let initial: SharedString = monocode_core::js::trim(name)
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".into())
        .into();
    let letter = {
        let theme = theme.clone();
        move || {
            div()
                .flex()
                .items_center()
                .justify_center()
                .size(u(size))
                .rounded_full()
                .border_1()
                .border_color(theme.colors.background_base)
                .bg(theme.content(0.1))
                .text_px((size * 0.45).max(8.))
                .medium()
                .text_color(theme.content(0.5))
                .child(initial.clone())
                .into_any_element()
        }
    };
    match avatar_url.filter(|url| !url.is_empty()) {
        Some(url) => img(SharedString::from(url.to_string()))
            .size(u(size))
            .flex_none()
            .rounded_full()
            .border_1()
            .border_color(theme.colors.background_base)
            .bg(theme.content(0.1))
            .object_fit(gpui::ObjectFit::Cover)
            .with_fallback(letter)
            .into_any_element(),
        None => letter(),
    }
}

/// Ids for the element tree of one chip.
pub fn chip_id(key: &str) -> ElementId {
    ElementId::Name(format!("user-link:{key}").into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed() -> ParsedWorkItem {
        ParsedWorkItem {
            pull_request: true,
            repo: "acme/widgets".into(),
            number: 73,
        }
    }

    #[test]
    fn the_chip_reads_pr_and_number() {
        assert_eq!(chip_text(&parsed()), "PR #73");
        let issue = ParsedWorkItem {
            pull_request: false,
            ..parsed()
        };
        assert_eq!(chip_text(&issue), "Issue #73");
        assert_eq!(
            chip_label(&parsed(), Some("Make work item links easier to scan")),
            "Open pull request #73 in acme/widgets: Make work item links easier to scan"
        );
        assert_eq!(chip_label(&issue, None), "Open issue #73 in acme/widgets");
    }

    #[test]
    fn opens_after_a_short_hover_delay() {
        assert_eq!(HOVER_OPEN_DELAY, Duration::from_millis(220));
        assert_eq!(HOVER_CLOSE_DELAY, Duration::from_millis(100));
    }

    #[test]
    fn summarizes_a_body_as_plain_text() {
        assert_eq!(
            plain_text_summary("Adds **compact chips** and a useful hover preview."),
            "Adds compact chips and a useful hover preview."
        );
        assert_eq!(
            plain_text_summary("![shot](a.png) See [the docs](https://x.y) <br/> ## Next"),
            "See the docs Next"
        );
    }

    #[test]
    fn colors_labels_from_six_hex_digits_only() {
        assert!(label_color("8b5cf6").is_some());
        assert!(label_color(" 8B5CF6 ").is_some());
        assert!(label_color("fff").is_none());
        assert!(label_color("purple").is_none());
    }

    #[test]
    fn reads_the_state_of_a_work_item() {
        let item = |state: &str, draft: bool| LinkWorkItem {
            title: "t".into(),
            state: state.into(),
            draft,
            updated: None,
            labels: Vec::new(),
            assignees: Vec::new(),
        };
        assert_eq!(work_item_status(true, None).1, "Pull request");
        assert_eq!(work_item_status(false, None).1, "Issue");
        assert_eq!(work_item_status(true, Some(&item("open", true))).1, "Draft");
        assert_eq!(
            work_item_status(true, Some(&item("merged", false))).2,
            StatusTone::Merged
        );
        assert_eq!(
            work_item_status(false, Some(&item("closed", false))).0,
            IconName::CircleX
        );
        assert_eq!(work_item_status(true, Some(&item("open", false))).1, "Open");
    }

    #[test]
    fn shows_the_branches_of_a_pull_request() {
        let details = LinkWorkItemDetails {
            base_ref_name: Some("main".into()),
            head_ref_name: Some("link-chips".into()),
            ..Default::default()
        };
        assert_eq!(
            branch_line(&details).as_deref(),
            Some("main \u{2190} link-chips")
        );
        assert_eq!(branch_line(&LinkWorkItemDetails::default()), None);
    }

    #[test]
    fn decodes_a_favicon_data_url() {
        let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        assert!(favicon_image(png).is_some());
        assert!(favicon_image("data:text/plain,hello").is_none());
        assert!(favicon_image("https://example.com/favicon.ico").is_none());
    }

    #[gpui::test]
    fn the_cache_asks_once_and_records_failures(cx: &mut gpui::TestAppContext) {
        let asked = Rc::new(std::cell::RefCell::new(Vec::new()));
        let sink = asked.clone();
        cx.update(|cx| {
            LinkPreviews::set_loader(cx, move |request, _| sink.borrow_mut().push(request));
            let request = LinkPreviewRequest::WorkItem {
                cwd: "/workspace/widgets".into(),
                repo: "acme/widgets".into(),
                pull_request: true,
                number: 73,
            };
            assert!(LinkPreviews::request(request.clone(), cx));
            assert!(!LinkPreviews::request(request, cx));
            let key = work_item_key(&parsed());
            LinkPreviews::resolve_work_item(&key, Err("no gh".into()), Err("no gh".into()), cx);
            assert!(LinkPreviews::work_item_failed(&key, cx));
            LinkPreviews::resolve_work_item(
                &key,
                Err("no gh".into()),
                Ok(LinkWorkItemDetails {
                    body: "Body".into(),
                    author: "ada".into(),
                    ..Default::default()
                }),
                cx,
            );
            assert!(!LinkPreviews::work_item_failed(&key, cx));
            assert_eq!(
                LinkPreviews::work_item_details(&key, cx).unwrap().author,
                "ada"
            );
        });
        assert_eq!(asked.borrow().len(), 1);
    }
}
