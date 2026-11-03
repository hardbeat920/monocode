use super::*;

impl Monos {
    pub(crate) fn search_conversations(&self) -> Vec<Entity<Conversation>> {
        self.chats
            .values()
            .chain(self.running.values().map(|r| &r.view))
            .cloned()
            .collect()
    }
    pub(crate) fn open_search_session(
        &mut self,
        session: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<Conversation>> {
        let id = self.roster.of_session(session)?.id.clone();
        self.open(&id, window, cx);
        self.chats.get(&id).cloned()
    }
}
