import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { AgentTranscript } from "../../src/features/sessions/ui/AgentTranscript";
import type { Block } from "../../src/features/sessions/model/session";
import {
  saveTranscriptAnchor,
  saveTranscriptLayout,
} from "../../src/features/settings/model/appearance";
import "../../src/styles/index.css";

saveTranscriptAnchor(false);
saveTranscriptLayout("chat");

// A generated prompt with headings, lists, links, inline code and nested fences.
// Use a four-backtick outer fence so the inner examples remain literal Markdown.
const prompt = Array.from(
  { length: 80 },
  (_, i) => `## Requirement ${i + 1}

You are implementing **a reliable application**. Inspect the existing files before making changes.

- Preserve the behavior of \`src/features/example.ts\` and explain any tradeoffs.
- Validate [the documentation](https://example.com/docs) and handle errors clearly.
- Return a concise report with the implementation, verification, and remaining work.

\`\`\`typescript
const requirement${i} = { enabled: true, retries: 3 };
\`\`\`
`,
).join("\n");
const response = `Here is the complete prompt:\n\n\`\`\`\`markdown\n${prompt}\n\`\`\`\``;

function Fixture() {
  const [reply, setReply] = useState<Block>({
    id: "reply",
    role: "assistant",
    text: "Ready.",
  });
  useEffect(() => {
    let timer: number | undefined;
    Object.assign(window, {
      markdownSource: prompt,
      showMarkdownExample: (text: string) =>
        setReply({ id: "reply", role: "assistant", text }),
      startMarkdownStream: () => {
        let end = 0;
        timer = window.setInterval(() => {
          end = Math.min(response.length, end + 800);
          setReply({
            id: "reply",
            role: "assistant",
            text: response.slice(0, end),
            streaming: end < response.length,
          });
          if (end === response.length) window.clearInterval(timer);
        }, 50);
      },
    });
    return () => window.clearInterval(timer);
  }, []);
  return (
    <AgentTranscript
      blocks={[
        {
          id: "user",
          role: "user",
          text: "Write a detailed prompt as fenced Markdown.",
        },
        reply,
      ]}
    />
  );
}

createRoot(document.getElementById("root")!).render(<Fixture />);
