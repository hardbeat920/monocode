//! A paired phone reads and triages the desktop Inbox. Everything it sees comes
//! from the Inbox the shell already runs in the background: a list is the
//! cached snapshot, answered at once, and a refresh runs behind it. Provider
//! reads (an item's text, a diff, checks, an image) run on the background
//! executor and never hold the UI thread. See docs/native/mobile-inbox-protocol.md.
use super::*;
use monocode_core::{
    inbox::Item,
    mobile::inbox::{self as wire, ListParams, Mark, ProjectRef, ReviewLine, SnapshotState, View},
};

/// At most this many items are announced in one batch; a flood of new items
/// after a long offline gap does not become a flood of alerts.
const MAX_PUSHES: usize = 5;

/// A pull-to-refresh waits for the fetch this long at most: 100 ticks of 200 ms.
const REFRESH_TICK: std::time::Duration = std::time::Duration::from_millis(200);
const REFRESH_WAIT_TICKS: usize = 100;

fn to_value(value: impl serde::Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}

/// A provider's error as a phone may read it: one line, nothing like a credential.
fn phone_error(error: String) -> String {
    wire::safe_error(&error)
}

/// The nth image of a body, fetched and made ready for the phone.
fn phone_image_at(body: &str, index: u64) -> Result<wire::MediaResponse, String> {
    let url = wire::image_urls(body)
        .into_iter()
        .nth(index as usize)
        .ok_or("That image is not in the item")?;
    let (bytes, mime) = monocode_core::inbox::phone_image(&url)?;
    wire::media_response(mime, &bytes)
}

fn now() -> i64 {
    monocode_core::time::now_ms() as i64
}

impl Shell {
    pub(super) fn mobile_inbox(
        &mut self,
        request: Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The Inbox the desktop polls in the background, made now if nothing
        // has opened it yet.
        self.ensure_background_inbox(window, cx);
        match request.method.as_str() {
            "inbox.list" => self.mobile_inbox_list(request, cx),
            "inbox.item" => self.mobile_inbox_item(request, cx),
            "inbox.diff" => self.mobile_inbox_diff(request, cx),
            "inbox.checks" => self.mobile_inbox_checks(request, cx),
            "inbox.mark" => self.mobile_inbox_mark(request, cx),
            "inbox.readAll" => self.mobile_inbox_read_all(request, cx),
            "inbox.review" | "inbox.reviewRemove" => {
                let result = self.mobile_inbox_review(&request, cx);
                request.reply(result);
            }
            "inbox.media" => self.mobile_inbox_media(request, cx),
            _ => request.reply(Err("Unsupported mobile operation".into())),
        }
    }

    /// The projects the phone knows, by catalog id.
    fn mobile_inbox_projects(&self, cx: &App) -> Vec<ProjectRef> {
        self.mobile_projects(cx)
            .into_iter()
            .map(|path| ProjectRef {
                id: project_id(&path),
                path,
            })
            .collect()
    }

    /// The listed item a request names, with the Inbox and service it came from.
    fn mobile_inbox_target(
        &self,
        params: &Value,
        cx: &App,
    ) -> Result<
        (
            Entity<crate::inbox::Inbox>,
            Item,
            monocode_core::inbox::Service,
        ),
        String,
    > {
        let key = string(params, "key").ok_or("Missing item")?;
        let inbox = self.inbox.clone().ok_or("The Inbox is not available")?;
        let view = inbox.read(cx).phone_view();
        let item = view
            .items
            .iter()
            .find(|item| item.key() == key)
            .cloned()
            .ok_or("That item is no longer in the Inbox")?;
        let service = view.service.clone();
        Ok((inbox, item, service))
    }

    /// An item as the phone lists it, from the Inbox's current state.
    fn mobile_inbox_slim(&self, item: &Item, cx: &App) -> Result<wire::InboxItem, String> {
        let inbox = self.inbox.as_ref().ok_or("The Inbox is not available")?;
        let projects = self.mobile_inbox_projects(cx);
        let view = inbox.read(cx).phone_view();
        let view = View {
            seen: view.seen,
            triage: view.triage,
            projects: &projects,
            viewers: view.viewers,
            now: now(),
        };
        Ok(wire::slim_item(item, &view, &self.mobile.inbox_avatars))
    }

    fn mobile_inbox_list(&mut self, request: Request, cx: &mut Context<Self>) {
        let params = match ListParams::parse(&request.params) {
            Ok(params) => params,
            Err(error) => return request.reply(Err(error)),
        };
        let Some(inbox) = self.inbox.clone() else {
            return request.reply(Err("The Inbox is not available".into()));
        };
        if params.refresh {
            // Pull to refresh: fetch now and answer when the fetch ends, or
            // after a while if a provider is slow, with what the Inbox has.
            inbox.update(cx, |inbox, cx| inbox.refresh_now_for_phone(cx));
            cx.spawn(async move |this, cx| {
                for _ in 0..REFRESH_WAIT_TICKS {
                    if !inbox.read_with(cx, |inbox, _| inbox.fetching()) {
                        break;
                    }
                    cx.background_executor().timer(REFRESH_TICK).await;
                }
                let _ = this.update(cx, |this, cx| {
                    this.mobile_inbox_answer(request, &params, cx)
                });
            })
            .detach();
            return;
        }
        inbox.update(cx, |inbox, cx| inbox.refresh_for_phone(cx));
        self.mobile_inbox_answer(request, &params, cx);
    }

    /// The list as the Inbox stands now.
    fn mobile_inbox_answer(
        &mut self,
        request: Request,
        params: &ListParams,
        cx: &mut Context<Self>,
    ) {
        let Some(inbox) = self.inbox.clone() else {
            return request.reply(Err("The Inbox is not available".into()));
        };
        let projects = self.mobile_inbox_projects(cx);
        let result = {
            let phone = inbox.read(cx).phone_view();
            wire::list_response(
                phone.items,
                params,
                &View {
                    seen: phone.seen,
                    triage: phone.triage,
                    projects: &projects,
                    viewers: phone.viewers,
                    now: now(),
                },
                &self.mobile.inbox_avatars,
                &SnapshotState {
                    fetched_at: phone.fetched_at,
                    interval_ms: phone.interval_ms,
                    connections: phone.connections,
                    errors: phone.errors,
                },
            )
        };
        request.reply(result.and_then(to_value));
    }

    fn mobile_inbox_item(&mut self, request: Request, cx: &mut Context<Self>) {
        let (_, item, service) = match self.mobile_inbox_target(&request.params, cx) {
            Ok(target) => target,
            Err(error) => return request.reply(Err(error)),
        };
        let background = cx.background_executor().clone();
        let details = {
            let (service, item) = (service.clone(), item.clone());
            background.spawn(async move { service.details(&item) })
        };
        let thread = {
            let (service, item) = (service.clone(), item.clone());
            background.spawn(async move { service.thread(&item) })
        };
        let checks = {
            let (service, item) = (service, item.clone());
            background.spawn(async move {
                if item.provider == "github" && item.kind == "pr" {
                    service.checks(&item).ok()
                } else {
                    None
                }
            })
        };
        cx.spawn(async move |this, cx| {
            let (details, thread, checks) = (details.await, thread.await, checks.await);
            let _ = this.update(cx, |this, cx| {
                if let (Err(error), Err(_)) = (&details, &thread) {
                    return request.reply(Err(phone_error(error.clone())));
                }
                let result = (|| {
                    let inbox = this.inbox.as_ref().ok_or("The Inbox is not available")?;
                    let projects = this.mobile_inbox_projects(cx);
                    let phone = inbox.read(cx).phone_view();
                    to_value(wire::item_response(
                        &item,
                        &View {
                            seen: phone.seen,
                            triage: phone.triage,
                            projects: &projects,
                            viewers: phone.viewers,
                            now: now(),
                        },
                        &this.mobile.inbox_avatars,
                        details.as_ref().ok(),
                        thread.as_ref().ok(),
                        checks.as_ref(),
                    ))
                })();
                request.reply(result);
            });
        })
        .detach();
    }

    fn mobile_inbox_diff(&mut self, request: Request, cx: &mut Context<Self>) {
        let (_, item, service) = match self.mobile_inbox_target(&request.params, cx) {
            Ok(target) => target,
            Err(error) => return request.reply(Err(error)),
        };
        let path = string(&request.params, "path");
        let work = cx.background_executor().spawn(async move {
            service
                .diff(&item)
                .and_then(|diff| wire::diff_response(&diff, path.as_deref()))
                .map_err(phone_error)
                .and_then(to_value)
        });
        cx.spawn(async move |_, _| request.reply(work.await))
            .detach();
    }

    fn mobile_inbox_checks(&mut self, request: Request, cx: &mut Context<Self>) {
        let (_, item, service) = match self.mobile_inbox_target(&request.params, cx) {
            Ok(target) => target,
            Err(error) => return request.reply(Err(error)),
        };
        if item.provider != "github" || item.kind != "pr" {
            return request.reply(Ok(json!({"checks": []})));
        }
        let work = cx.background_executor().spawn(async move {
            service
                .checks(&item)
                .map(|checks| wire::checks_response(&checks))
                .map_err(phone_error)
                .and_then(to_value)
        });
        cx.spawn(async move |_, _| request.reply(work.await))
            .detach();
    }

    fn mobile_inbox_mark(&mut self, request: Request, cx: &mut Context<Self>) {
        let result = (|| {
            let mark = Mark::parse(&request.params)?;
            let (inbox, ..) = self.mobile_inbox_target(&request.params, cx)?;
            let key = string(&request.params, "key").unwrap_or_default();
            let item = inbox.update(cx, |inbox, cx| inbox.phone_mark(&key, mark, cx))?;
            to_value(json!({"item": self.mobile_inbox_slim(&item, cx)?}))
        })();
        request.reply(result);
    }

    /// `inbox.readAll`: everything the list shows goes read.
    fn mobile_inbox_read_all(&mut self, request: Request, cx: &mut Context<Self>) {
        let result = (|| {
            let params = ListParams::parse(&request.params)?;
            let inbox = self.inbox.clone().ok_or("The Inbox is not available")?;
            let projects = self.mobile_inbox_projects(cx);
            let marked = inbox.update(cx, |inbox, cx| inbox.phone_read_all(&params, &projects, cx));
            to_value(json!({"marked": marked}))
        })();
        request.reply(result);
    }

    /// `inbox.action`: what the desktop's pull request buttons do. Under a
    /// command ID, so a retry after a lost reply can't merge twice.
    pub(super) fn mobile_inbox_action(
        &mut self,
        request: Request,
        finish: impl FnOnce(&mut Shell, Request, Result<Value, String>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let prepared = (|| {
            let (inbox, item, service) = self.mobile_inbox_target(&request.params, cx)?;
            let name = request.params["action"].as_str().unwrap_or_default();
            let action = wire::pull_request_action(&item, name)?;
            Ok::<_, String>((inbox, item, service, action))
        })();
        let (inbox, item, service, action) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => return finish(self, request, Err(error)),
        };
        let key = item.key();
        let work = cx
            .background_executor()
            .spawn(async move { service.action(&item, action) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                let result = result.map_err(phone_error).and_then(|updated| {
                    let item = inbox.update(cx, |inbox, cx| inbox.phone_acted(&key, updated, cx));
                    to_value(wire::ActionResponse {
                        actions: wire::actions(&item),
                        queued: wire::queued(&item, action),
                        item: this.mobile_inbox_slim(&item, cx)?,
                    })
                });
                finish(this, request, result);
            });
        })
        .detach();
    }

    /// `inbox.comment`: a comment on an item, or a reply to a thread entry's
    /// `replyTo`. Under a command ID, so a retry can't post it twice.
    pub(super) fn mobile_inbox_comment(
        &mut self,
        request: Request,
        finish: impl FnOnce(&mut Shell, Request, Result<Value, String>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let prepared = (|| {
            let body = string(&request.params, "body")
                .map(|body| body.trim().to_owned())
                .filter(|body| !body.is_empty())
                .ok_or("Write a comment first")?;
            let (inbox, item, service) = self.mobile_inbox_target(&request.params, cx)?;
            Ok::<_, String>((body, inbox, item, service))
        })();
        let (body, inbox, item, service) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => return finish(self, request, Err(error)),
        };
        let reply = string(&request.params, "replyTo").unwrap_or_default();
        let key = item.key();
        let work = cx
            .background_executor()
            .spawn(async move { service.comment(&item, body, reply) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                let result = result.map_err(phone_error).map(|_| {
                    inbox.update(cx, |inbox, cx| inbox.phone_discussed(&key, cx));
                    json!({})
                });
                finish(this, request, result);
            });
        })
        .detach();
    }

    /// The pull request a review request names.
    fn mobile_inbox_reviewed(
        &self,
        params: &Value,
        cx: &App,
    ) -> Result<
        (
            Entity<crate::inbox::Inbox>,
            Item,
            monocode_core::inbox::Service,
        ),
        String,
    > {
        let target = self.mobile_inbox_target(params, cx)?;
        if !wire::reviewable(&target.1) {
            return Err("Line comments are for GitHub pull requests".into());
        }
        Ok(target)
    }

    /// `inbox.review` and `inbox.reviewRemove`: the pending comments, which are
    /// the desktop's own. Removing one twice changes nothing, so neither needs
    /// a command ID.
    fn mobile_inbox_review(
        &mut self,
        request: &Request,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let (inbox, item, _) = self.mobile_inbox_reviewed(&request.params, cx)?;
        let key = item.key();
        let comments = if request.method == "inbox.reviewRemove" {
            let id = request.params["id"].as_u64().ok_or("Missing comment")?;
            inbox.update(cx, |inbox, _| inbox.phone_review_remove(&key, id))
        } else {
            inbox.update(cx, |inbox, _| inbox.phone_review(&key))
        };
        to_value(wire::ReviewResponse { comments })
    }

    /// `inbox.reviewAdd`: under a command ID, so a retry adds it once.
    pub(super) fn mobile_inbox_review_add(
        &mut self,
        params: &Value,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let line = ReviewLine::parse(params)?;
        let (inbox, item, _) = self.mobile_inbox_reviewed(params, cx)?;
        let key = item.key();
        let comments = inbox.update(cx, |inbox, _| inbox.phone_review_add(&key, line));
        to_value(wire::ReviewResponse { comments })
    }

    /// `inbox.reviewSubmit`: the pending comments as one review that only
    /// comments, as the desktop posts it. Under a command ID, so a retry can't
    /// post it twice.
    pub(super) fn mobile_inbox_review_submit(
        &mut self,
        request: Request,
        finish: impl FnOnce(&mut Shell, Request, Result<Value, String>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let prepared = (|| {
            let (inbox, item, service) = self.mobile_inbox_reviewed(&request.params, cx)?;
            let key = item.key();
            let comments = inbox.update(cx, |inbox, _| inbox.phone_review_start(&key))?;
            Ok::<_, String>((inbox, item, service, key, comments))
        })();
        let (inbox, item, service, key, comments) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => return finish(self, request, Err(error)),
        };
        let work = cx.background_executor().spawn({
            let comments = comments.clone();
            async move { service.submit_review(&item, "", &comments) }
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                let posted = result.is_ok();
                let remaining = inbox.update(cx, |inbox, cx| {
                    inbox.phone_review_finished(&key, &comments, posted, cx)
                });
                let result = result.map_err(phone_error).and_then(|_| {
                    to_value(wire::ReviewResponse {
                        comments: remaining,
                    })
                });
                finish(this, request, result);
            });
        })
        .detach();
    }

    /// An avatar or an image in the item's body or one of its comments, which
    /// the host fetches itself from an address it knows; the phone names only a
    /// key or an index.
    fn mobile_inbox_media(&mut self, request: Request, cx: &mut Context<Self>) {
        let params = request.params.clone();
        let avatar_key = string(&params, "avatarKey");
        let index = params["index"].as_u64();
        let comment = string(&params, "commentId").zip(params["commentIndex"].as_u64());
        let work = match (avatar_key, index, comment) {
            (Some(key), None, None) => {
                let Some(url) = self.mobile.inbox_avatars.url(&key) else {
                    return request.reply(Err("That avatar is no longer available".into()));
                };
                if !url.starts_with("https://") {
                    return request.reply(Err("That avatar is not available".into()));
                }
                cx.background_executor().spawn(async move {
                    let png = monocode_core::inbox::avatar_png(&url)?;
                    wire::media_response("image/png", &png)
                })
            }
            (None, Some(index), None) => {
                let (_, item, service) = match self.mobile_inbox_target(&params, cx) {
                    Ok(target) => target,
                    Err(error) => return request.reply(Err(error)),
                };
                cx.background_executor()
                    .spawn(async move { phone_image_at(&service.details(&item)?.body, index) })
            }
            (None, None, Some((id, index))) => {
                let (_, item, service) = match self.mobile_inbox_target(&params, cx) {
                    Ok(target) => target,
                    Err(error) => return request.reply(Err(error)),
                };
                cx.background_executor().spawn(async move {
                    let thread = service.thread(&item)?;
                    let body = wire::comment_body(&thread, &id)
                        .ok_or("That comment is not in the item")?;
                    phone_image_at(body, index)
                })
            }
            _ => return request.reply(Err("Ask for one avatar or one image".into())),
        };
        cx.spawn(async move |_, _| {
            request.reply(work.await.map_err(phone_error).and_then(to_value))
        })
        .detach();
    }

    /// `inbox.start`: a session about an item, started as `session.start` is
    /// and under its command ID. Reached through `mobile_command`, so a retry
    /// with the same ID gets the first session back.
    pub(super) fn mobile_inbox_start(
        &mut self,
        request: Request,
        finish: impl FnOnce(&mut Shell, Request, Result<Value, String>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let params = request.params.clone();
        let prepared = (|| {
            let ask = match params["mode"].as_str() {
                Some("work") => false,
                Some("ask") => true,
                _ => return Err("Choose work or ask".to_string()),
            };
            let project = string(&params, "projectId").ok_or("Choose a project")?;
            let (_, item, service) = self.mobile_inbox_target(&params, cx)?;
            Ok::<_, String>((ask, project, item, service))
        })();
        let (ask, project, item, service) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => return finish(self, request, Err(error)),
        };
        let background = cx.background_executor().clone();
        let details = {
            let (service, item) = (service.clone(), item.clone());
            background.spawn(async move { service.details(&item) })
        };
        let thread = background.spawn({
            let item = item.clone();
            async move { service.thread(&item) }
        });
        cx.spawn(async move |this, cx| {
            let (details, thread) = (details.await, thread.await);
            let _ = this.update(cx, |this, cx| {
                // The link in the prompt still names the item when a read fails.
                let body = details.map(|d| d.body).unwrap_or_default();
                let discussion = thread
                    .map(|thread| {
                        thread
                            .comments
                            .iter()
                            .map(|c| format!("{}: {}", c.author, c.body))
                            .collect::<Vec<_>>()
                            .join("\n\n")
                    })
                    .unwrap_or_default();
                let mut text = item.prompt(&clip(&format!("{body}\n\n{discussion}")), ask);
                if let Some(extra) = string(&request.params, "text")
                    .map(|extra| extra.trim().to_owned())
                    .filter(|extra| !extra.is_empty())
                {
                    text = format!("{text}\n\nExtra instructions:\n{}", clip(&extra));
                }
                let mut start = json!({"projectId": project, "text": text});
                for key in ["harness", "model"] {
                    if let Some(value) = string(&request.params, key) {
                        start[key] = json!(value);
                    }
                }
                this.mobile_start(request.with_params(start), finish, cx);
            });
        })
        .detach();
    }

    /// New items that need the user, sent to phones that take the `inbox`
    /// category. Not while the desktop shows the Inbox in front of the user.
    pub(in crate::shell) fn notify_mobile_inbox(
        &mut self,
        items: Vec<Item>,
        cx: &mut Context<Self>,
    ) {
        if self.mobile.host.is_none() {
            return;
        }
        if self.window_active
            && self
                .inbox
                .as_ref()
                .is_some_and(|inbox| inbox.read(cx).is_active())
        {
            return;
        }
        let done = |item: &Item| {
            self.inbox.as_ref().is_some_and(|inbox| {
                let view = inbox.read(cx).phone_view();
                view.triage.is_done(&item.key())
                    || view.triage.snoozed_until(&item.key(), now()).is_some()
            })
        };
        for item in items
            .iter()
            .filter(|item| wire::pushes(item) && !done(item))
            .take(MAX_PUSHES)
        {
            let (title, body, repo) = wire::notice(item);
            self.send_mobile_notice(
                "inbox",
                notify::Notice {
                    title,
                    subtitle: Some(repo).filter(|repo| !repo.is_empty()),
                    body,
                    data: json!({"kind": "inbox", "key": item.key()}),
                },
                cx,
            );
        }
    }
}

/// Keeps a prompt's pieces well under the message limit.
fn clip(text: &str) -> String {
    const MAX: usize = 40_000;
    if text.len() <= MAX {
        return text.to_owned();
    }
    let mut end = MAX;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}
