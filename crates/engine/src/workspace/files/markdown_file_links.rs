//! Port of src/features/files/model/markdownFileLinks.ts. The TypeScript
//! was a remark plugin that rewrote link URLs in the markdown tree; here it
//! is the rewrite for one URL, which the markdown renderer calls per link.

use monocode_core::js::encode_uri_component;

use crate::workspace::paths::resolve_workspace_file_reference;

/// The href a local file link becomes: an absolute, percent-encoded path
/// with its `:line:column` kept. `None` leaves the URL as it is.
pub fn workspace_file_link_href(url: &str, cwd: Option<&str>) -> Option<String> {
    let file = resolve_workspace_file_reference(url, cwd)?;
    let path = if file.path.starts_with('/') {
        file.path.clone()
    } else {
        format!("/{}", file.path)
    };
    let mut href = path
        .split('/')
        .map(encode_uri_component)
        .collect::<Vec<_>>()
        .join("/");
    // Keep a UNC path a path, not a protocol-relative web origin.
    if let Some(rest) = href.strip_prefix("//") {
        href = format!("/%2F{rest}");
    }
    if let Some(target) = file.navigation {
        href.push_str(&format!(":{}", target.line));
        if let Some(column) = target.column {
            href.push_str(&format!(":{column}"));
        }
    }
    Some(href)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_local_links_and_keeps_web_links() {
        assert_eq!(
            workspace_file_link_href("src/a b.ts:3:2", Some("/repo")).as_deref(),
            Some("/repo/src/a%20b.ts:3:2")
        );
        assert_eq!(
            workspace_file_link_href("https://example.com", Some("/repo")),
            None
        );
        assert_eq!(
            workspace_file_link_href("C:/repo/a.ts", None).as_deref(),
            Some("/C%3A/repo/a.ts")
        );
    }
}
