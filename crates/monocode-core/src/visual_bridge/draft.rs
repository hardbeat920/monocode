//! The agent's design draft: an HTML page (or a local dev server) drawn as a
//! card in the conversation and inspected through screenshots.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::mpsc;

pub mod images;
pub mod shared;

pub const SCRIPT_HOSTS: [&str; 7] = [
    "cdn.jsdelivr.net",
    "cdnjs.cloudflare.com",
    "unpkg.com",
    "esm.sh",
    "cdn.skypack.dev",
    "ajax.googleapis.com",
    "cdn.tailwindcss.com",
];
pub const STYLE_HOSTS: [&str; 9] = [
    "cdn.jsdelivr.net",
    "cdnjs.cloudflare.com",
    "unpkg.com",
    "esm.sh",
    "ajax.googleapis.com",
    "fonts.googleapis.com",
    "fonts.gstatic.com",
    "fonts.bunny.net",
    "api.fontshare.com",
];
pub const FONT_HOSTS: [&str; 8] = [
    "cdn.jsdelivr.net",
    "cdnjs.cloudflare.com",
    "unpkg.com",
    "fonts.googleapis.com",
    "fonts.gstatic.com",
    "fonts.bunny.net",
    "cdn.fontshare.com",
    "api.fontshare.com",
];

/// `https://a https://b`, as a CSP source list.
pub fn csp_sources(hosts: &[&str]) -> String {
    hosts
        .iter()
        .map(|host| format!("https://{host}"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub const WIDTHS: std::ops::RangeInclusive<u32> = 240..=1600;
pub const HEIGHTS: std::ops::RangeInclusive<u32> = 120..=1600;
pub const DEFAULT_WIDTH: u32 = 640;
pub const DEFAULT_HEIGHT: u32 = 480;

/// A page without its embedded images.
const MAX_MARKUP_BYTES: usize = 512 * 1024;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DraftAction {
    #[default]
    Snapshot,
    Click {
        reference: String,
    },
    Type {
        text: String,
        #[serde(default)]
        replace: bool,
        /// The field to type into; without it, the focused one.
        #[serde(default)]
        reference: Option<String>,
    },
    Resume,
    /// Patch the page's `mc.state`, the way its own controls would.
    Set {
        values: serde_json::Map<String, Value>,
    },
    /// Wait for the person to send feedback or hand the draft back.
    Wait {
        #[serde(default)]
        seconds: Option<u32>,
    },
    Close,
}

/// The longest one wait lasts; the agent calls again to keep waiting.
pub const MAX_WAIT_SECONDS: u32 = 30;
pub const DEFAULT_WAIT_SECONDS: u32 = 25;

#[derive(Clone, Debug, PartialEq)]
pub enum DraftContent {
    Html(String),
    /// A dev server on this machine.
    Url(String),
}

pub struct DraftRequest {
    pub session: i64,
    pub draft: Option<String>,
    pub content: Option<DraftContent>,
    pub action: DraftAction,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub light: Option<bool>,
    /// A short name shown above a new draft in the conversation.
    pub title: Option<String>,
    /// The identifier a new draft takes, chosen by the bridge so the
    /// conversation's card can name it.
    pub id: Option<String>,
    pub reply: mpsc::Sender<Result<DraftReply, String>>,
}

/// Whether `url` points at a dev server on this machine, without credentials.
pub fn local_url(url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|_| "Invalid url")?;
    let local = matches!(
        parsed.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
    );
    if !matches!(parsed.scheme(), "http" | "https")
        || !local
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err("Drafts open http(s) dev servers on localhost, without credentials".into());
    }
    Ok(())
}

impl DraftRequest {
    /// Closes a draft without anyone waiting on the answer.
    pub fn close(session: i64, draft: String) -> Self {
        Self {
            session,
            draft: Some(draft),
            content: None,
            action: DraftAction::Close,
            width: None,
            height: None,
            light: None,
            title: None,
            id: None,
            reply: mpsc::channel().0,
        }
    }

    pub fn parse(
        session: i64,
        input: &Value,
        reply: mpsc::Sender<Result<DraftReply, String>>,
    ) -> Result<Self, String> {
        let optional_string = |key| {
            input
                .get(key)
                .filter(|v| !v.is_null())
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| format!("{key} must be a string"))
                })
                .transpose()
        };
        let draft = optional_string("draft")?;
        let html = optional_string("html")?;
        let url = optional_string("url")?;
        let content = match (html, url) {
            (Some(_), Some(_)) => return Err("Pass html or url, not both".into()),
            (Some(html), None) => {
                check_html(&html)?;
                Some(DraftContent::Html(html))
            }
            (None, Some(url)) => {
                local_url(&url)?;
                Some(DraftContent::Url(url))
            }
            (None, None) => None,
        };
        if draft.is_none() && content.is_none() {
            return Err(
                "Pass html or a localhost url to start a draft, or draft to continue one".into(),
            );
        }
        if draft
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 96)
        {
            return Err("Invalid draft identifier".into());
        }
        let action: DraftAction = input
            .get("action")
            .filter(|v| !v.is_null())
            .map(|value| serde_json::from_value(value.clone()))
            .transpose()
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        if !matches!(action, DraftAction::Snapshot) && (draft.is_none() || content.is_some()) {
            return Err("Actions need an existing draft and no new content".into());
        }
        match &action {
            DraftAction::Type { text, .. } if text.len() > 4096 => {
                return Err("Text exceeds 4096 bytes".into());
            }
            DraftAction::Click { reference }
            | DraftAction::Type {
                reference: Some(reference),
                ..
            } if reference.is_empty() || reference.len() > 96 => {
                return Err("Invalid element reference".into());
            }
            DraftAction::Set { values } if values.is_empty() => {
                return Err("values must name at least one state key".into());
            }
            DraftAction::Set { values }
                if Value::Object(values.clone()).to_string().len() > 4000 =>
            {
                return Err("values exceed 4000 bytes".into());
            }
            DraftAction::Wait {
                seconds: Some(seconds),
            } if !(1..=MAX_WAIT_SECONDS).contains(seconds) => {
                return Err(format!("seconds must be 1 to {MAX_WAIT_SECONDS}"));
            }
            _ => {}
        }
        let dimension = |key, range: std::ops::RangeInclusive<u32>| {
            input
                .get(key)
                .filter(|v| !v.is_null())
                .map(|v| {
                    v.as_u64()
                        .and_then(|v| u32::try_from(v).ok())
                        .filter(|v| range.contains(v))
                        .ok_or_else(|| format!("Invalid {key}"))
                })
                .transpose()
        };
        let title = optional_string("title")?
            .map(|title| title.trim().to_owned())
            .filter(|title| !title.is_empty());
        if title.as_ref().is_some_and(|t| t.chars().count() > 160) {
            return Err("Title exceeds 160 characters".into());
        }
        let light = match input.get("appearance") {
            None | Some(Value::Null) => None,
            Some(value) if value == "light" => Some(true),
            Some(value) if value == "dark" => Some(false),
            _ => return Err("appearance must be light or dark".into()),
        };
        Ok(Self {
            session,
            draft,
            content,
            action,
            width: dimension("width", WIDTHS)?,
            height: dimension("height", HEIGHTS)?,
            light,
            title,
            id: None,
            reply,
        })
    }
}

pub enum DraftReply {
    Snapshot(Box<Snapshot>),
    Closed { draft: String },
}

impl DraftReply {
    pub(super) fn value(self) -> Value {
        match self {
            Self::Snapshot(snapshot) => {
                use base64::Engine;
                let mut value = serde_json::to_value(&snapshot).expect("Snapshot is serializable");
                value["png"] = base64::engine::general_purpose::STANDARD
                    .encode(&snapshot.png)
                    .into();
                value
            }
            Self::Closed { draft } => json!({"draft":draft,"closed":true}),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    #[serde(skip)]
    pub png: Vec<u8>,
    pub draft: String,
    pub width: u32,
    pub height: u32,
    pub content_height: u32,
    pub loaded: bool,
    pub console: Vec<LogEntry>,
    pub diagnostics: Diagnostics,
    pub elements: Vec<Element>,
    pub text: String,
    /// The person took over the draft; inspect it, but resume before acting.
    pub user_control: bool,
    pub url: Option<String>,
    pub title: Option<String>,
    /// What the person sent from the page since the agent last looked: notes
    /// pinned to elements. Empty when nothing was sent.
    pub feedback: Vec<Feedback>,
    /// What the page itself sent with `mc.send` when the person used one of
    /// its controls, oldest first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<String>,
    /// What the person did in the page by hand, oldest first.
    pub user_actions: Vec<String>,
    /// Why a wait returned: "feedback", "message", "handed back" or "timeout".
    pub event: Option<String>,
    /// Things worth fixing that the page itself reveals, such as low
    /// contrast, tiny tap targets and missing labels.
    pub issues: Vec<String>,
    /// The values of the page's own controls (`mc.state`), when it uses them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<serde_json::Value>,
    /// How this version of the page lays out at other common widths, measured
    /// out of sight while it loaded.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub layouts: Vec<Layout>,
}

/// The page laid out at one viewport width.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    pub width: u32,
    /// The height the content needs at that width.
    pub height: u32,
    /// How far the content runs past the viewport's right edge.
    pub horizontal_overflow: u32,
}

/// The widths a new version of a page is also laid out at: a phone, a tablet
/// and a laptop.
pub const LAYOUT_WIDTHS: [u32; 3] = [375, 768, 1280];

/// A note the person pinned to an element of the page.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Feedback {
    pub note: String,
    pub tag: String,
    /// The element's label or text.
    pub name: String,
    /// Where the pin sits, as a percentage of the page.
    pub x: f64,
    pub y: f64,
    /// The element the pin was placed on, so another screen whose page is laid
    /// out at another width can put it on the same element. It travels with
    /// the note only as far as the pin; the agent reads the percentages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Anchor>,
}

/// Where in the page a pin sits: the element, as the child indexes from
/// `<body>` down to it ("0.2.1"), and the point inside its box as a fraction
/// of its width and height. Every screen loads the same page, so the path
/// names the same element on each.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Anchor {
    pub path: String,
    pub ax: f64,
    pub ay: f64,
}

impl Anchor {
    /// The longest path a page may report; deeper elements are pinned by
    /// percentage alone.
    pub const MAX_PATH: usize = 160;

    /// The anchor if it is one a page could have sent: a short path of child
    /// indexes and a point inside the box.
    pub fn checked(self) -> Option<Self> {
        let path_ok = !self.path.is_empty()
            && self.path.len() <= Self::MAX_PATH
            && self.path.split('.').all(|step| {
                !step.is_empty() && step.len() <= 4 && step.bytes().all(|b| b.is_ascii_digit())
            });
        let inside = |v: f64| v.is_finite() && (0. ..=1.).contains(&v);
        (path_ok && inside(self.ax) && inside(self.ay)).then_some(self)
    }
}

impl Feedback {
    /// The person's notes as the message the agent reads when it was not
    /// waiting for them.
    pub fn message(
        title: &str,
        feedback: &[Self],
        messages: &[String],
        actions: &[String],
    ) -> String {
        let mut text = format!("Feedback on the draft \u{201c}{title}\u{201d}:\n");
        for message in messages {
            text.push_str(&format!("From the page: {message}\n"));
        }
        for (n, item) in feedback.iter().enumerate() {
            text.push_str(&format!(
                "{}. On <{}> \u{201c}{}\u{201d} (about {:.0}% across, {:.0}% down): {}\n",
                n + 1,
                item.tag,
                item.name,
                item.x,
                item.y,
                item.note
            ));
        }
        if !actions.is_empty() {
            text.push_str(&format!("In the page I also: {}.\n", actions.join("; ")));
        }
        if !feedback.is_empty() {
            text.push_str(
                "The pins are numbered in the page; look at it again before changing it.",
            );
        }
        text
    }
}

#[derive(Serialize)]
pub struct LogEntry {
    pub level: String,
    pub text: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Element {
    #[serde(rename = "ref")]
    pub reference: String,
    pub name: String,
    pub role: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub value: Option<String>,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    pub ready: bool,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub horizontal_overflow: u32,
    pub vertical_overflow: u32,
    pub failed_images: u32,
}

/// The page runs under a strict CSP, so unlisted code and unembedded images
/// fail silently in the view. Say so before showing it instead.
pub fn check_html(html: &str) -> Result<(), String> {
    // Embedded images count against their own budget, not the page's.
    let images: usize = embedded_images()
        .find_iter(html)
        .map(|image| image.len())
        .sum();
    if html.len().saturating_sub(images) > MAX_MARKUP_BYTES {
        return Err("Draft HTML exceeds 512 KiB".into());
    }
    if html.len() > images::MAX_HTML_BYTES {
        return Err("Draft HTML with its images exceeds 10 MiB".into());
    }
    if html.trim().is_empty() {
        return Err("HTML must not be empty".into());
    }
    let lower = html.to_ascii_lowercase();
    // Code and styles may come from a short list of CDNs over https; the
    // page's CSP admits exactly those origins.
    let allowed = |url: &str, hosts: &[&str], what: &str| -> Result<(), String> {
        if !(url.starts_with("http") || url.starts_with("//")) {
            return Ok(());
        }
        let host = url.strip_prefix("https://").map(|rest| {
            rest.split(['/', '?', '#', ':', '"', '\'', ' ', ')'])
                .next()
                .unwrap_or("")
        });
        match host {
            Some(host) if hosts.contains(&host) => Ok(()),
            _ => Err(format!(
                "{what} may only load from https://{}",
                hosts.join(", https://")
            )),
        }
    };
    let values = |tag: &str, attr: &str| -> Vec<String> {
        lower
            .match_indices(tag)
            .filter_map(|(at, _)| {
                let end = lower[at..].find('>').map_or(lower.len(), |n| at + n);
                let element = &lower[at..end];
                let i = element.find(attr)?;
                let rest = element[i + attr.len()..].trim_start_matches(['=', ' ', '"', '\'']);
                Some(rest.to_owned())
            })
            .collect()
    };
    for url in values("<script", "src") {
        allowed(&url, &SCRIPT_HOSTS, "Scripts")?;
    }
    for url in values("<link", "href") {
        allowed(&url, &STYLE_HOSTS, "Stylesheets")?;
    }
    for (at, _) in lower.match_indices("@import") {
        let rest = lower[at + 7..]
            .trim_start()
            .trim_start_matches("url(")
            .trim_start_matches(['"', '\'', ' ']);
        allowed(rest, &STYLE_HOSTS, "@import")?;
    }
    if values("<img", "src")
        .iter()
        .any(|url| url.starts_with("http") || url.starts_with("//"))
    {
        return Err(
            "This image was not embedded; give it as an https URL or an absolute file path to an image file"
                .into(),
        );
    }
    Ok(())
}

fn embedded_images() -> &'static regex::Regex {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    PATTERN.get_or_init(|| {
        regex::Regex::new(r"data:image/[a-zA-Z0-9.+-]+;base64,[A-Za-z0-9+/=]+").unwrap()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: Value) -> Result<DraftRequest, String> {
        let (reply, _) = mpsc::channel();
        DraftRequest::parse(7, &input, reply)
    }

    #[test]
    fn waits_are_bounded_and_need_a_draft() {
        assert!(matches!(
            parse(json!({"draft":"a","action":{"kind":"wait"}}))
                .unwrap()
                .action,
            DraftAction::Wait { seconds: None }
        ));
        assert!(parse(json!({"draft":"a","action":{"kind":"wait","seconds":30}})).is_ok());
        for bad in [0, 31] {
            assert!(parse(json!({"draft":"a","action":{"kind":"wait","seconds":bad}})).is_err());
        }
        assert!(parse(json!({"html":"<p>x</p>","action":{"kind":"wait"}})).is_err());
    }

    #[test]
    fn feedback_reads_as_a_message_with_where_each_note_sits() {
        let notes = [
            Feedback {
                note: "Make this blue".into(),
                tag: "button".into(),
                name: "Choose plan".into(),
                x: 42.4,
                y: 80.,
                at: None,
            },
            Feedback {
                note: "Too tight".into(),
                tag: "h2".into(),
                name: "Team".into(),
                ..Feedback::default()
            },
        ];
        let text = Feedback::message(
            "Pricing",
            &notes,
            &["Chose variant B".into()],
            &["clicked Details".into()],
        );
        assert!(text.contains("From the page: Chose variant B\n"));
        assert!(text.contains("\u{201c}Pricing\u{201d}"));
        assert!(text.contains("1. On <button> \u{201c}Choose plan\u{201d} (about 42% across, 80% down): Make this blue"));
        assert!(text.contains("2. On <h2>"));
        assert!(text.contains("In the page I also: clicked Details."));
        let sent = Feedback::message("Pricing", &[], &["Use this one".into()], &[]);
        assert!(sent.contains("From the page: Use this one"));
        assert!(!sent.contains("pins"));
    }

    #[test]
    fn set_patches_named_state_within_bounds() {
        let request = parse(
            json!({"draft":"a","action":{"kind":"set","values":{"speed":0.25,"phase":"open"}}}),
        )
        .unwrap();
        let DraftAction::Set { values } = request.action else {
            panic!("Expected a set action");
        };
        assert_eq!(values["speed"], 0.25);
        for bad in [
            json!({"draft":"a","action":{"kind":"set","values":{}}}),
            json!({"draft":"a","action":{"kind":"set","values":[1]}}),
            json!({"draft":"a","action":{"kind":"set"}}),
            json!({"draft":"a","action":{"kind":"set","values":{"x":"y".repeat(4001)}}}),
            json!({"html":"<p>x</p>","action":{"kind":"set","values":{"x":1}}}),
        ] {
            assert!(parse(bad.clone()).is_err(), "{bad}");
        }
    }

    #[test]
    fn requests_need_content_or_a_draft_and_a_valid_action() {
        assert!(parse(json!({"html":"<button>Hi</button>"})).is_ok());
        let request = parse(
            json!({"draft":"draft-7-1","action":{"kind":"type","text":"Hello","replace":true}}),
        )
        .unwrap();
        assert_eq!(request.session, 7);
        assert!(matches!(
            request.action,
            DraftAction::Type { replace: true, .. }
        ));
        assert!(matches!(
            parse(json!({"draft":"a","action":{"kind":"type","text":"Ada","reference":"k3-2"}}))
                .unwrap()
                .action,
            DraftAction::Type { reference: Some(r), replace: false, .. } if r == "k3-2"
        ));
        for input in [
            json!({}),
            json!({"draft":1}),
            json!({"html":"<p>Hi</p>","url":"http://localhost:3000"}),
            json!({"html":"<p>Hi</p>","action":{"kind":"close"}}),
            json!({"draft":"a","action":{"kind":"click"}}),
            json!({"draft":"a","action":{"kind":"click","reference":"a","text":"ignored"}}),
            json!({"draft":"a","action":{"kind":"keep","title":"Pricing"}}),
            json!({"draft":"a","width":0}),
            json!({"draft":"a","appearance":"sepia"}),
            json!({"draft":"a","action":{"kind":"type","text":"x".repeat(4097)}}),
            json!({"draft":"a","action":{"kind":"type","text":"x","reference":""}}),
        ] {
            assert!(
                parse(input.clone()).is_err(),
                "Accepted invalid draft input: {input}"
            );
        }
    }

    #[test]
    fn only_local_dev_servers_open_as_urls() {
        for url in [
            "http://localhost:3000",
            "https://localhost/app?x=1",
            "http://127.0.0.1:5173/",
        ] {
            assert!(parse(json!({"url":url})).is_ok(), "{url}");
        }
        for url in [
            "https://example.com",
            "http://user:pw@localhost:3000",
            "file:///etc/passwd",
            "http://localhost.evil.com",
            "javascript:alert(1)",
        ] {
            assert!(parse(json!({"url":url})).is_err(), "{url}");
        }
    }

    #[test]
    fn pages_may_only_load_code_from_listed_hosts() {
        assert!(check_html("<p>See https://example.com for more</p>").is_ok());
        assert!(check_html(
            r#"<script src="https://cdn.jsdelivr.net/npm/chart.js"></script><link rel=stylesheet href="https://fonts.googleapis.com/css2?family=Inter">"#
        )
        .is_ok());
        for bad in [
            r#"<script src="https://evil.example/x.js"></script>"#,
            r#"<script src="//cdn.jsdelivr.net/x.js"></script>"#,
            r#"<link rel=stylesheet href="https://evil.example/x.css">"#,
            r#"<style>@import url("https://evil.example/x.css")</style>"#,
            r#"<img src="https://example.com/a.png">"#,
            "   ",
        ] {
            assert!(check_html(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn pages_may_call_network_apis_and_write_about_them() {
        for page in [
            "<script>fetch('https://api.example.com/x')</script>",
            "<script>new WebSocket('wss://example.com')</script>",
            "<script>new XMLHttpRequest()</script>",
            "<p>The fetch() call and the WebSocket API</p>",
        ] {
            assert!(check_html(page).is_ok(), "{page}");
        }
    }

    #[test]
    fn embedded_images_do_not_count_against_the_markup_budget() {
        let image = format!("data:image/png;base64,{}", "A".repeat(200_000));
        assert!(check_html(&format!("<img src=\"{image}\">")).is_ok());
        assert!(check_html(&format!("<p>{}</p>", "x".repeat(MAX_MARKUP_BYTES))).is_err());
    }
}
