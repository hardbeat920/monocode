use gpui::{AssetSource, Result, SharedString};
use std::borrow::Cow;
pub(crate) struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let bytes:&'static [u8]=match path{
   "pencil-edit-02"=>include_bytes!("../assets/hugeicons/pencil-edit-02.svg"),
   "login-01"=>include_bytes!("../assets/hugeicons/login-01.svg"),
   "logout-01"=>include_bytes!("../assets/hugeicons/logout-01.svg"),
   "smart-phone"=>include_bytes!("../assets/hugeicons/smart-phone.svg"),
   "delete-02"=>include_bytes!("../assets/hugeicons/delete-02.svg"),
   "account-user"=>include_bytes!("../assets/hugeicons/account-user.svg"),
   "queue-list"=>include_bytes!("../assets/hugeicons/queue-list.svg"),
   "queue-steer"=>include_bytes!("../assets/hugeicons/queue-steer.svg"),
   "queue-pause"=>include_bytes!("../assets/hugeicons/queue-pause.svg"),
   "queue-play"=>include_bytes!("../assets/hugeicons/queue-play.svg"),
   "play"=>include_bytes!("../assets/hugeicons/play.svg"),
   "test-tube"=>include_bytes!("../assets/hugeicons/test-tube.svg"),
   "hammer"=>include_bytes!("../assets/hugeicons/hammer.svg"),
   "rocket"=>include_bytes!("../assets/hugeicons/rocket.svg"),
   "bug"=>include_bytes!("../assets/hugeicons/bug.svg"),
   "package"=>include_bytes!("../assets/hugeicons/package.svg"),
   "smile"=>include_bytes!("../assets/hugeicons/smile.svg"),
   "sticky-note"=>include_bytes!("../assets/hugeicons/sticky-note.svg"),
   "pipette"=>include_bytes!("../assets/hugeicons/pipette.svg"),
   "square"=>include_bytes!("../assets/hugeicons/square.svg"),
   "cancel-01"=>include_bytes!("../assets/hugeicons/cancel-01.svg"),
   "window-maximize"=>include_bytes!("../assets/hugeicons/window-maximize.svg"),
   "window-restore"=>include_bytes!("../assets/hugeicons/window-restore.svg"),
   "dashboard"=>include_bytes!("../assets/hugeicons/dashboard.svg"),
   "file"=>include_bytes!("../assets/hugeicons/file.svg"),
   "mono-send"=>include_bytes!("../assets/hugeicons/mono-send.svg"),
   "mono-arrow-up"=>include_bytes!("../assets/hugeicons/mono-arrow-up.svg"),
   "mono-attach"=>include_bytes!("../assets/hugeicons/mono-attach.svg"),
   "gauge"=>include_bytes!("../assets/hugeicons/gauge.svg"),
   "chatting"=>include_bytes!("../assets/hugeicons/chatting.svg"),
   "check-circle"=>include_bytes!("../assets/hugeicons/check-circle.svg"),
   // The title tab's done check, at the React tab's 2 stroke.
   "check-circle-2"=>include_bytes!("../assets/hugeicons/check-circle-2.svg"),
   "circle-x"=>include_bytes!("../assets/hugeicons/circle-x.svg"),
   "loader-03"=>include_bytes!("../assets/hugeicons/loader-03.svg"),
   "alert-circle"=>include_bytes!("../assets/hugeicons/alert-circle.svg"),
   "circle-dashed-hi"=>include_bytes!("../assets/hugeicons/circle-dashed-hi.svg"),
   "chart-square"=>include_bytes!("../assets/hugeicons/chart-square.svg"),
   "command-btw"=>include_bytes!("../assets/hugeicons/command-btw.svg"),
   "command-visualize"=>include_bytes!("../assets/hugeicons/command-visualize.svg"),
   "command-compact"=>include_bytes!("../assets/hugeicons/command-compact.svg"),
   "message-multiple"=>include_bytes!("../assets/hugeicons/message-multiple.svg"),
   "folder-01"=>include_bytes!("../assets/hugeicons/folder-01.svg"),
   "plus"=>include_bytes!("../assets/plus.svg"),
   "tool-branch"=>include_bytes!("../assets/tool-branch.svg"),
   "response-copy"=>include_bytes!("../assets/hugeicons/response-copy.svg"),
   "response-note"=>include_bytes!("../assets/hugeicons/response-note.svg"),
   "response-activity"=>include_bytes!("../assets/hugeicons/response-activity.svg"),
   "response-handoff"=>include_bytes!("../assets/hugeicons/response-handoff.svg"),
   "response-opinion"=>include_bytes!("../assets/hugeicons/response-opinion.svg"),
   "response-metrics"=>include_bytes!("../assets/hugeicons/response-metrics.svg"),
   "close"=>include_bytes!("../assets/close.svg"),
   "sidebar"=>include_bytes!("../assets/sidebar.svg"),
   // The sidebar toggle mirrored, for the right panel.
   "sidebar-right"=>include_bytes!("../assets/sidebar-right.svg"),
   "search"=>include_bytes!("../assets/search.svg"),
   "case-sensitive"=>include_bytes!("../assets/hugeicons/case-sensitive.svg"),
   "whole-word"=>include_bytes!("../assets/hugeicons/whole-word.svg"),
   "regex"=>include_bytes!("../assets/hugeicons/regex.svg"),
   "folder"=>include_bytes!("../assets/folder.svg"),
   "open-folder"=>include_bytes!("../assets/open-folder.svg"),
   "chevron-down"=>include_bytes!("../assets/chevron-down.svg"),
   "chevrons-up-down"=>include_bytes!("../assets/chevrons-up-down.svg"),
   "chevron-left"=>include_bytes!("../assets/chevron-left.svg"),
   "chevron-right"=>include_bytes!("../assets/chevron-right.svg"),
   "inbox"=>include_bytes!("../assets/inbox.svg"),
   "circle-dot"=>include_bytes!("../assets/hugeicons/circle-dot.svg"),
   "inbox-github"=>include_bytes!("../assets/inbox-github.svg"),
   "inbox-linear"=>include_bytes!("../assets/inbox-linear.svg"),
   "inbox-jira"=>include_bytes!("../assets/inbox-jira.svg"),
   "inbox-gitlab"=>include_bytes!("../assets/inbox-gitlab.svg"),
   "inbox-gitlab-color.svg"=>include_bytes!("../assets/inbox-gitlab-color.svg"),
   "inbox-azuredevops"=>include_bytes!("../assets/inbox-azuredevops.svg"),

   "note"=>include_bytes!("../assets/note.svg"),
   "zap"=>include_bytes!("../assets/zap.svg"),
   "filter"=>include_bytes!("../assets/filter.svg"),
   "settings"=>include_bytes!("../assets/settings.svg"),
   // Hugeicons as the React app draws them: their own caps and joins,
   // at its 1.75 stroke (checks at 2 and 2.25).
   "ai-idea"=>include_bytes!("../assets/hugeicons/ai-idea.svg"),
   "app-window"=>include_bytes!("../assets/hugeicons/app-window.svg"),
   "archive"=>include_bytes!("../assets/hugeicons/archive.svg"),
   "bell-off"=>include_bytes!("../assets/hugeicons/bell-off.svg"),
   "check"=>include_bytes!("../assets/hugeicons/check.svg"),
   "check-2"=>include_bytes!("../assets/hugeicons/check-2.svg"),
   "cursor-magic"=>include_bytes!("../assets/hugeicons/cursor-magic.svg"),
   "file-add"=>include_bytes!("../assets/hugeicons/file-add.svg"),
   "folder-open"=>include_bytes!("../assets/hugeicons/folder-open.svg"),
   "folder-tree"=>include_bytes!("../assets/hugeicons/folder-tree.svg"),
   "image-add"=>include_bytes!("../assets/hugeicons/image-add.svg"),
   "pencil-edit"=>include_bytes!("../assets/hugeicons/pencil-edit.svg"),
   "pin"=>include_bytes!("../assets/hugeicons/pin.svg"),
   "pin-off"=>include_bytes!("../assets/hugeicons/pin-off.svg"),
   "share"=>include_bytes!("../assets/hugeicons/share.svg"),
   "shield-alert"=>include_bytes!("../assets/hugeicons/shield-alert.svg"),
   "sliders"=>include_bytes!("../assets/hugeicons/sliders.svg"),
   "sparkles"=>include_bytes!("../assets/hugeicons/sparkles.svg"),
   "square-lock"=>include_bytes!("../assets/hugeicons/square-lock.svg"),
   "star"=>include_bytes!("../assets/hugeicons/star.svg"),
   "star-filled"=>include_bytes!("../assets/hugeicons/star-filled.svg"),
   "trash"=>include_bytes!("../assets/hugeicons/trash.svg"),
   "chevron-up"=>include_bytes!("../assets/hugeicons/chevron-up.svg"),
   "unfold-vertical"=>include_bytes!("../assets/hugeicons/unfold-vertical.svg"),
   "comment-add"=>include_bytes!("../assets/hugeicons/comment-add.svg"),
   "git-compare"=>include_bytes!("../assets/hugeicons/git-compare.svg"),
   "more-horizontal"=>include_bytes!("../assets/hugeicons/more-horizontal.svg"),
   "loader"=>include_bytes!("../assets/hugeicons/loader.svg"),
   "refresh-cw"=>include_bytes!("../assets/hugeicons/refresh-cw.svg"),
   "eye"=>include_bytes!("../assets/hugeicons/eye.svg"),
   "chevron-down-2"=>include_bytes!("../assets/hugeicons/chevron-down-2.svg"),
   "wand"=>include_bytes!("../assets/hugeicons/wand.svg"),
   "x"=>include_bytes!("../assets/hugeicons/x.svg"),
   "terminal"=>include_bytes!("../assets/hugeicons/terminal.svg"),
   "panel-bottom"=>include_bytes!("../assets/hugeicons/panel-bottom.svg"),
   "panel-top"=>include_bytes!("../assets/hugeicons/panel-top.svg"),
   "panel-left"=>include_bytes!("../assets/hugeicons/panel-left.svg"),
   "panel-right"=>include_bytes!("../assets/hugeicons/panel-right.svg"),
   "file-script"=>include_bytes!("../assets/hugeicons/file-script.svg"),
   "git-branch"=>include_bytes!("../assets/hugeicons/git-branch.svg"),
   "clock"=>include_bytes!("../assets/hugeicons/clock.svg"),
   "coffee"=>include_bytes!("../assets/hugeicons/coffee.svg"),
   "list-bullet"=>include_bytes!("../assets/hugeicons/list-bullet.svg"),
   "file-diff"=>include_bytes!("../assets/hugeicons/file-diff.svg"),
   "minus-sign"=>include_bytes!("../assets/hugeicons/minus-sign.svg"),
   "undo"=>include_bytes!("../assets/hugeicons/undo.svg"),
   "cloud-upload"=>include_bytes!("../assets/hugeicons/cloud-upload.svg"),
   "git-merge"=>include_bytes!("../assets/hugeicons/git-merge.svg"),
   "git-commit"=>include_bytes!("../assets/hugeicons/git-commit.svg"),
   "git-pull-request-draft"=>include_bytes!("../assets/hugeicons/git-pull-request-draft.svg"),
   "git-pull-request-closed"=>include_bytes!("../assets/hugeicons/git-pull-request-closed.svg"),
   "git-pull-request"=>include_bytes!("../assets/hugeicons/git-pull-request.svg"),
   "external-link"=>include_bytes!("../assets/hugeicons/external-link.svg"),
   "folder-add"=>include_bytes!("../assets/hugeicons/folder-add.svg"),
   "fold-vertical"=>include_bytes!("../assets/hugeicons/fold-vertical.svg"),
   "git-branch-2"=>include_bytes!("../assets/hugeicons/git-branch-2.svg"),
   "git-branch-hi"=>include_bytes!("../assets/hugeicons/git-branch-hi.svg"),
   "git-branch-dot"=>include_bytes!("../assets/hugeicons/git-branch-dot.svg"),
   // Settings: its rail, rows and page actions (SettingsRail.tsx, SettingsView.tsx).
   "internet"=>include_bytes!("../assets/hugeicons/internet.svg"),
   "palette"=>include_bytes!("../assets/hugeicons/palette.svg"),
   "keyboard"=>include_bytes!("../assets/hugeicons/keyboard.svg"),
   "message-square"=>include_bytes!("../assets/hugeicons/message-square.svg"),
   "bot"=>include_bytes!("../assets/hugeicons/bot.svg"),
   "globe"=>include_bytes!("../assets/hugeicons/globe.svg"),
   "ghost"=>include_bytes!("../assets/hugeicons/ghost.svg"),
   "arrow-left"=>include_bytes!("../assets/hugeicons/arrow-left.svg"),
   "rotate-ccw"=>include_bytes!("../assets/hugeicons/rotate-ccw.svg"),
   "search-01"=>include_bytes!("../assets/hugeicons/search-01.svg"),
   "add-01"=>include_bytes!("../assets/hugeicons/add-01.svg"),
   "arrow-down-circle"=>include_bytes!("../assets/hugeicons/arrow-down-circle.svg"),
   "inbox-hi"=>include_bytes!("../assets/hugeicons/inbox-hi.svg"),
   "chevron-down-hi"=>include_bytes!("../assets/hugeicons/chevron-down-hi.svg"),
   "check-hi"=>include_bytes!("../assets/hugeicons/check-hi.svg"),
   "monocode.png"=>include_bytes!("../assets/monocode.png"),
   "codex.svg"=>include_bytes!("../assets/codex.svg"),
   "claude.svg"=>include_bytes!("../assets/claude.svg"),
   "cursor.svg"=>include_bytes!("../assets/cursor.svg"),
   "grok.svg"=>include_bytes!("../assets/grok.svg"),
   "opencode.svg"=>include_bytes!("../assets/opencode.svg"),
   "antigravity.svg"=>include_bytes!("../assets/antigravity.svg"),
   "pi.svg"=>include_bytes!("../assets/pi.svg"),
   "omp.svg"=>include_bytes!("../assets/omp.svg"),
   "fx.svg"=>include_bytes!("../assets/fx.svg"),
   "hermes.svg"=>include_bytes!("../assets/hermes.svg"),
   "devin.svg"=>include_bytes!("../assets/devin.svg"),
   _ if path.starts_with("mi/")=>return Ok(crate::file_icons::svg(path).map(Cow::Borrowed)),
   _=>return Ok(icon_body(path).map(|body|Cow::Owned(format!("<svg xmlns='http://www.w3.org/2000/svg' width='24' height='24' viewBox='0 0 24 24' fill='none' stroke='white' stroke-width='1.6' stroke-linecap='round' stroke-linejoin='round'>{body}</svg>").into_bytes())))
  };
        Ok(Some(Cow::Borrowed(bytes)))
    }
    fn list(&self, _: &str) -> Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}
fn icon_body(path: &str) -> Option<&'static str> {
    Some(match path {
        "circle-alert" => "<circle cx='12' cy='12' r='9'/><path d='M12 7v6m0 4h.01'/>",
        "refresh" => {
            "<path d='M20 7v5h-5M4 17v-5h5'/><path d='M6 6a8 8 0 0 1 13 2l1 4M4 12l1 4a8 8 0 0 0 13 2'/>"
        }
        "plus" => "<path d='M12 5v14M5 12h14'/>",
        "close" => "<path d='m7 7 10 10M17 7 7 17'/>",
        "grip-vertical" => {
            "<circle cx='9' cy='5' r='1'/><circle cx='9' cy='12' r='1'/><circle cx='9' cy='19' r='1'/><circle cx='15' cy='5' r='1'/><circle cx='15' cy='12' r='1'/><circle cx='15' cy='19' r='1'/>"
        }
        "sidebar" => "<rect x='3' y='4' width='18' height='16' rx='3'/><path d='M9 4v16'/>",
        "search" => "<circle cx='10.5' cy='10.5' r='6.5'/><path d='m16 16 4.5 4.5'/>",
        "folder" => {
            "<path d='M3 8V6a2 2 0 0 1 2-2h5l2 3h7a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z'/>"
        }
        "chevron-down" => "<path d='m7 10 5 5 5-5'/>",
        "chevron-right" => "<path d='m10 7 5 5-5 5'/>",
        "chat" => {
            "<path d='M21 11.5a8.5 8.5 0 0 1-8.5 8.5H4l-1 1v-9.5A8.5 8.5 0 0 1 11.5 3h1A8.5 8.5 0 0 1 21 11.5Z'/><path d='M8 10h8M8 14h5'/>"
        }
        "arrow-up" => "<path d='M12 19V5m-6 6 6-6 6 6'/>",
        "arrow-down" => "<path d='M12 5v14m-6-6 6 6 6-6'/>",
        "stop" => "<rect x='6' y='6' width='12' height='12' rx='2' fill='white' stroke='none'/>",
        "lock" => {
            "<rect x='5' y='10' width='14' height='11' rx='3'/><path d='M8 10V7a4 4 0 0 1 8 0v3'/>"
        }
        "open-folder" => {
            "<path d='M3 8V6a2 2 0 0 1 2-2h5l2 3h7a2 2 0 0 1 2 2v3M3 8v10a2 2 0 0 0 2 2h13l4-8H7l-4 8'/>"
        }
        // Hugeicons ArrowUp01 at the composer submit's 2.25 stroke.
        "send" => "<path d='M18 15s-4.42-6-6-6-6 6-6 6' stroke-width='2.25'/>",
        "git-branch" => {
            "<path d='M7 19h6c2.83 0 4.24 0 5.12-.88C19 17.24 19 15.83 19 13v-3m0 0c.7 0 2.01 1.99 2.5 2.5M19 10c-.7 0-2.01 1.99-2.5 2.5M5 7v10' stroke-width='1.75'/><circle cx='5' cy='5' r='2' stroke-width='1.75'/><circle cx='19' cy='5' r='2' stroke-width='1.75'/><circle cx='5' cy='19' r='2' stroke-width='1.75'/>"
        }
        // ContextMeter's empty 14px ring (radius 6, stroke 2) on a 24 grid.
        "context-ring" => "<circle cx='12' cy='12' r='10.29' stroke-width='3.43'/>",
        "window" => "<rect x='3' y='4' width='18' height='16' rx='3'/><path d='M3 9h18'/>",
        // lucide's Maximize2, Shield and Pencil, as in QuickComposer.tsx.
        "maximize-2" => "<path d='M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7'/>",
        "shield" => {
            "<path d='M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z'/>"
        }
        "pencil" => {
            "<path d='M21.17 6.81a1 1 0 0 0-3.98-3.98L3.84 16.17a2 2 0 0 0-.5.83l-1.32 4.35a.5.5 0 0 0 .62.62l4.35-1.32a2 2 0 0 0 .83-.5zM15 5l4 4'/>"
        }
        "copy" => {
            "<path d='M14 4H6a2 2 0 0 0-2 2v8'/><rect x='8' y='8' width='12' height='12' rx='2'/>"
        }
        "code" => "<path d='m8 7-5 5 5 5m8-10 5 5-5 5m-3-14-2 18'/>",
        "minus" => "<path d='M6 12h12'/>",
        "pencil-line" => "<path d='M12 20h9M16.5 3.5a2.12 2.12 0 0 1 3 3L9 17l-4 1 1-4Z'/>",
        "circle-dashed" => {
            "<path d='M12 3a9 9 0 0 1 4.5 1.2m3.3 3.3A9 9 0 0 1 21 12m-1.2 4.5a9 9 0 0 1-3.3 3.3M12 21a9 9 0 0 1-4.5-1.2m-3.3-3.3A9 9 0 0 1 3 12m1.2-4.5a9 9 0 0 1 3.3-3.3'/>"
        }
        _ => return None,
    })
}
