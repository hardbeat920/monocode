//! Read selected Mono work metadata in the background, once per source revision.
use super::*;
type Revision = (u64, u64, u64);
#[derive(Default)]
pub(super) struct Cache {
    revisions: HashMap<String, Revision>,
    pending: HashSet<String>,
    checked: HashMap<String, std::time::Instant>,
    pub(super) documents: HashMap<String, mono::Document>,
    pub(super) loading_documents: HashSet<String>,
}
impl Monos {
    pub(super) fn open_document(&mut self, id: &str, cx: &mut Context<Self>) {
        // Visual documents carry their whole body from the chat projection.
        if self.current().is_some_and(|mono| {
            mono.documents.iter().any(|d| {
                d.id == id
                    && (d.kind == "visual"
                        || !mono.desktop_document_ids.iter().any(|source| source == id))
            })
        }) {
            return;
        }
        if !self.work.loading_documents.insert(id.into()) {
            return;
        }
        let store = self.store.clone();
        let id = id.to_owned();
        let target = id.clone();
        let task = cx
            .background_executor()
            .spawn(async move { locked(&store)?.mono_document(&target) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.work.loading_documents.remove(&id);
                match result {
                    Ok(Some(document)) => {
                        this.work.documents.insert(id.clone(), document);
                        this.documents.remove(&id);
                        this.html_documents.remove(&id);
                    }
                    Ok(None) => {
                        this.work.documents.remove(&id);
                        this.error = Some("Document unavailable. It may have been deleted.".into());
                    }
                    Err(error) => {
                        this.error = Some(format!("Could not open this document: {error}"))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn refresh_work(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.work.pending.contains(id)
            || self
                .work
                .checked
                .get(id)
                .is_some_and(|at| at.elapsed() < Duration::from_secs(1))
        {
            return;
        }
        let Some(session) = self.roster.get(id).and_then(|m| m.session_id) else {
            return;
        };
        self.work.pending.insert(id.into());
        self.work
            .checked
            .insert(id.into(), std::time::Instant::now());
        let previous = self.work.revisions.get(id).copied();
        let store = self.store.clone();
        let id = id.to_owned();
        let work = cx.background_executor().spawn(async move {
            let db = locked(&store)?;
            let revision = db.mono_work_revision(session)?;
            let data = if revision.is_some() && revision != previous {
                db.mono_work(session)?
            } else {
                None
            };
            Ok::<_, String>((revision, data))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.work.pending.remove(&id);
                let (revision, data) = match result {
                    Ok(value) => value,
                    Err(error) => {
                        this.error = Some(format!("Could not refresh this Mono’s work: {error}"));
                        cx.notify();
                        return;
                    }
                };
                if let Some(revision) = revision {
                    this.work.revisions.insert(id.clone(), revision);
                }
                let Some((documents, launches)) = data else {
                    return;
                };
                let Some(mono) = this
                    .roster
                    .get_mut(&id)
                    .filter(|m| m.session_id == Some(session))
                else {
                    return;
                };
                let old_documents = mono.desktop_document_ids.clone();
                let changed = mono.apply_work(documents, launches).unwrap_or(false);
                if changed {
                    // Replace readers whose body changed while they were open.
                    for id in old_documents
                        .iter()
                        .chain(mono.documents.iter().map(|d| &d.id))
                    {
                        this.documents.remove(id);
                        this.html_documents.remove(id);
                        this.visual_documents.remove(id);
                        this.work.documents.remove(id);
                    }
                    if this.active.as_deref() == Some(&id)
                        && let Some(Route::Document(document)) = this.route.clone()
                    {
                        this.open_document(&document, cx);
                    }
                    this.configure_chat(&id, cx);
                    this.refresh_cards(cx);
                    cx.emit(Changed);
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
