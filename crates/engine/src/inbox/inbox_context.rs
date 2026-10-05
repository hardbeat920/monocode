//! Port of src/features/inbox/model/inboxContext.ts: load a tracker issue's
//! description before opening a session, including from list actions.

use futures::FutureExt;
use futures::future::BoxFuture;

use super::client::InboxClient;
use super::text::non_empty_trimmed;
use super::types::{InboxItem, InboxProvider};

impl InboxClient {
    /// `inboxTrackerDescription`: `body` for GitHub and repository items or
    /// when given, otherwise the Jira or Linear description.
    pub fn inbox_tracker_description(
        &self,
        item: &InboxItem,
        body: Option<String>,
    ) -> BoxFuture<'static, Result<Option<String>, String>> {
        if !item.is_tracker() || body.is_some() {
            return futures::future::ready(Ok(body)).boxed();
        }
        let client = self.clone();
        if item.provider == InboxProvider::Jira {
            let key = non_empty_trimmed(item.identifier.as_deref()).map(str::to_string);
            return async move {
                let key = key.ok_or("Missing Jira issue key")?;
                let details = match client.peek_jira_issue_details(&key) {
                    Some(details) => details,
                    None => client.jira_issue_details(&key).await?,
                };
                Ok(Some(details.body))
            }
            .boxed();
        }
        let id = item.id.clone().filter(|id| !id.is_empty());
        async move {
            let id = id.ok_or("Missing Linear issue")?;
            let details = match client.peek_linear_issue_details(&id) {
                Some(details) => details,
                None => client.linear_issue_details(&id).await?,
            };
            Ok(Some(details.body))
        }
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use futures::FutureExt;
    use gpui::TestAppContext;

    use crate::inbox::client::test_support::{client, settle_future};
    use crate::inbox::client_tests::{jira_handler, jira_inbox_item};
    use crate::inbox::github_tasks::inbox_start_draft;

    #[gpui::test]
    fn provides_the_jira_description_and_identifier_to_sessions(cx: &mut TestAppContext) {
        let (client, backend) = client(cx, jira_handler);
        let issue = jira_inbox_item();
        let description =
            settle_future(cx, client.inbox_tracker_description(&issue, None)).unwrap();
        assert_eq!(
            backend.calls_to("jira_issue_details"),
            [serde_json::json!({ "key": "ENG-42" })]
        );
        let draft = inbox_start_draft(&issue, description.as_deref());
        assert!(draft.contains("ENG-42 Fix auth"));
        assert!(draft.contains("Reproduction steps"));

        backend.clear_calls();
        let provided = client
            .inbox_tracker_description(&issue, Some("Provided description".into()))
            .now_or_never()
            .unwrap();
        assert_eq!(provided, Ok(Some("Provided description".into())));
        assert!(backend.calls().is_empty());

        let mut keyless = issue.clone();
        keyless.identifier = Some(String::new());
        let missing = client
            .inbox_tracker_description(&keyless, None)
            .now_or_never()
            .unwrap();
        assert_eq!(missing, Err("Missing Jira issue key".into()));
    }
}
