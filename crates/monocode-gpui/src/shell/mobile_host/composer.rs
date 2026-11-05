//! A paired phone shows and changes the text left in a thread's composer.
//! The desktop store holds the one copy, with a revision per change, so a
//! phone's write from an older revision cannot wipe newer text. See
//! docs/native/composer-drafts-sync.md.
use super::*;
use monocode_core::store::{ComposerText, ComposerWrite};

/// How long a phone's read waits for the text to change before answering.
const READ_WAIT: Duration = Duration::from_secs(15);
const READ_POLL: Duration = Duration::from_millis(250);

impl Shell {
    /// `composer.read`: the thread's composer text and its revision. With
    /// `since`, it answers once the revision moves past that one, or after a
    /// while with the same one.
    pub(super) fn mobile_composer_read(&mut self, request: Request, cx: &mut Context<Self>) {
        let session = match self.mobile_composer_session(&request.params, cx) {
            Ok(session) => session,
            Err(error) => return request.reply(Err(error)),
        };
        let since = request.params["since"].as_u64();
        let store = self.store.clone();
        let executor = cx.background_executor().clone();
        cx.background_executor()
            .spawn(async move {
                let started = executor.now();
                let read = || locked(&store).and_then(|s| s.composer_text(session));
                let text = loop {
                    match read() {
                        Ok(text)
                            if since == Some(text.rev) && executor.now() - started < READ_WAIT => {}
                        result => break result,
                    }
                    executor.timer(READ_POLL).await;
                };
                request.reply(text.map(|text| text.json()));
            })
            .detach();
    }

    /// `composer.write`: the phone's text, based on revision `base`. Answers
    /// whether it was kept and the desktop's text and revision afterwards.
    pub(super) fn mobile_composer_write(
        &mut self,
        params: &Value,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let session = self.mobile_composer_session(params, cx)?;
        let text = params["text"].as_str().ok_or("Missing text")?;
        let base = params["base"].as_u64().unwrap_or(0);
        Ok(reply(&self.write_composer(session, text, base, cx)?))
    }

    /// Text for a thread's composer, based on revision `base`.
    pub(in crate::shell) fn write_composer(
        &mut self,
        session: i64,
        text: &str,
        base: u64,
        cx: &mut Context<Self>,
    ) -> Result<ComposerWrite, String> {
        if text.len() > MAX_MESSAGE {
            return Err("Draft is too long".into());
        }
        match self.sessions.get(&session).cloned() {
            Some(view) => view.update(cx, |view, cx| view.mobile_composer_write(text, base, cx)),
            None => {
                let store = locked(&self.store)?;
                let write = store.write_composer_text(session, text, base)?;
                // The sidebar's Draft mark, and the text the thread opens with.
                if store.composer_draft(session)?.is_some() {
                    self.composer_draft_sessions.insert(session);
                } else {
                    self.composer_draft_sessions.remove(&session);
                }
                cx.notify();
                Ok(write)
            }
        }
    }

    fn mobile_composer_session(&self, params: &Value, cx: &App) -> Result<i64, String> {
        let conversation = string(params, "conversationId").unwrap_or_default();
        match self.mobile_target(&conversation, cx)? {
            Target::Session(session) => Ok(session),
            Target::Mono(_) => Err("A Mono's chat does not share its composer".into()),
        }
    }
}

fn reply(write: &ComposerWrite) -> Value {
    let (accepted, text): (bool, &ComposerText) = match write {
        ComposerWrite::Saved(text) => (true, text),
        ComposerWrite::Stale(text) => (false, text),
    };
    let mut reply = text.json();
    reply["accepted"] = accepted.into();
    reply
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn the_phone_and_the_desktop_composer_share_one_draft(cx: &mut gpui::TestAppContext) {
        let (window, _, _) = crate::shell::tests::window(cx);
        let open = window
            .update(cx, |shell, _, _| {
                shell.workspace.active.expect("a conversation is open")
            })
            .unwrap();
        let conversation = window
            .update(cx, |shell, _, _| shell.mobile_session_id(open))
            .unwrap();
        let write = |cx: &mut gpui::TestAppContext, text: &str, base: Value| {
            window
                .update(cx, |shell, _, cx| {
                    let params =
                        json!({"conversationId": conversation, "text": text, "base": base});
                    shell.mobile_composer_write(&params, cx).unwrap()
                })
                .unwrap()
        };
        let composer = |cx: &mut gpui::TestAppContext| {
            window
                .update(cx, |shell, _, cx| {
                    shell.sessions[&open]
                        .read(cx)
                        .input
                        .read(cx)
                        .content
                        .to_string()
                })
                .unwrap()
        };

        // The phone's text shows in the open composer, and is the saved draft.
        let first = write(cx, "from the phone", json!(0));
        assert_eq!(first["accepted"], true);
        cx.run_until_parked();
        assert_eq!(composer(cx), "from the phone");

        // Typed on the desktop: a phone write from the old revision is refused
        // and told the newer text.
        window
            .update(cx, |shell, _, cx| {
                shell.sessions[&open].update(cx, |view, cx| {
                    view.input.update(cx, |input, cx| {
                        input.set_text("from the desktop".into(), cx)
                    });
                    view.flush_composer_draft(cx);
                })
            })
            .unwrap();
        cx.run_until_parked();
        let stale = write(cx, "phone, offline", first["rev"].clone());
        assert_eq!(stale["accepted"], false);
        assert_eq!(stale["text"], "from the desktop");
        assert_eq!(composer(cx), "from the desktop");

        // Even against the current revision, the person typing here keeps
        // their text for a moment.
        let held = write(cx, "phone, now", stale["rev"].clone());
        assert_eq!(held["accepted"], false);
        cx.executor().advance_clock(Duration::from_secs(3));
        let kept = write(cx, "phone, now", stale["rev"].clone());
        assert_eq!(kept["accepted"], true);
        cx.run_until_parked();
        assert_eq!(composer(cx), "phone, now");

        // A phone send based on that text clears it here before it goes in.
        window
            .update(cx, |shell, _, cx| {
                shell.sessions[&open].update(cx, |view, cx| {
                    view.clear_composer_text_sent_from_phone("phone, now", kept["rev"].as_u64(), cx)
                })
            })
            .unwrap();
        cx.run_until_parked();
        assert_eq!(composer(cx), "");
        let read = window
            .update(cx, |shell, _, _| {
                locked(&shell.store).unwrap().composer_text(open).unwrap()
            })
            .unwrap();
        assert_eq!(read, ComposerText::default());

        let mono = json!({"conversationId": "mono:none", "text": "x", "base": 0});
        window
            .update(cx, |shell, _, cx| {
                assert!(shell.mobile_composer_write(&mono, cx).is_err())
            })
            .unwrap();
    }
}
