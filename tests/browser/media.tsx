import { createRoot } from "react-dom/client";
import { AgentMarkdown } from "../../src/features/sessions/ui/AgentMarkdown";
import "../../src/styles/index.css";

// The README screenshot stands in for both a fetched GitHub attachment and a
// screenshot an agent saved on disk. Paths with "missing" fail to load.
const screenshot = fetch("/docs/screenshot.jpg").then((response) =>
  response.arrayBuffer(),
);
Object.assign(window, {
  __TAURI_INTERNALS__: {
    invoke: (_command: string, args: { url?: string; path?: string }) =>
      (args.url ?? args.path ?? "").includes("missing")
        ? Promise.reject(new Error("not found"))
        : screenshot,
    transformCallback: () => 0,
  },
});

const inbox = `## UI

| Before | After |
| --- | --- |
| <img width="390" alt="before" src="https://github.com/user-attachments/assets/aaaaaaaa-1111-2222-3333-444444444444" /> | <img width="390" alt="after" src="https://github.com/user-attachments/assets/aaaaaaaa-1111-2222-3333-555555555555" /> |

| Release | Blob |
| --- | --- |
| ![Previous timer](https://github.com/acme/web/releases/download/v1/before.png) | ![New timer](https://github.com/acme/web/blob/main/docs/after.png?raw=true) |

<img width="390" alt="solo" src="https://github.com/user-attachments/assets/aaaaaaaa-1111-2222-3333-666666666666" />

![Hosted elsewhere](https://example.com/shot.png)

![Local file](/tmp/never-read.png)
`;

const chat = `Here is the screenshot:

![Settings screen](/tmp/settings.png)

![](/tmp/missing.png)
`;

createRoot(document.getElementById("root")!).render(
  <>
    <div data-testid="inbox">
      <AgentMarkdown text={inbox} allowRemoteMedia />
    </div>
    <div data-testid="chat">
      <AgentMarkdown text={chat} cwd="/tmp" />
    </div>
  </>,
);
