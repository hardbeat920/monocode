import { captureReaderAnchor, readerAnchorTop } from "./readerAnchor";
import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { AgentTranscript } from "../../src/features/sessions/ui/AgentTranscript";
import {
  PooledTranscript,
  TranscriptPool,
  TranscriptPoolOutlet,
  TRANSCRIPT_POOL_LIMIT,
} from "../../src/features/sessions/ui/TranscriptPool";
import type { Block } from "../../src/features/sessions/model/session";
import {
  saveTranscriptAnchor,
  saveTranscriptLayout,
} from "../../src/features/settings/model/appearance";
import "../../src/styles/index.css";

saveTranscriptAnchor(true);
saveTranscriptLayout("chat");
const history: Block[] = Array.from({ length: 100 }, (_, i): Block[] => [
  { id: `user-${i}`, role: "user", text: `Question ${i}` },
  {
    id: `answer-${i}`,
    role: "assistant",
    text:
      `Answer ${i}. ` +
      "Settled text with **formatting** and `code`. ".repeat(8),
  },
]).flat();
const prompt: Block = {
  id: "latest-prompt",
  role: "user",
  text: "Explain this in detail.",
  startedAt: Date.now(),
};
const burst = Array.from(
  { length: 24 },
  (_, i) =>
    `Paragraph ${i}. ` +
    "A provider can deliver a large burst of text while other sessions are working. ".repeat(
      4,
    ),
).join("\n\n");

// Synthetic prose keeps the nested Markdown shapes needed by the scroll tests.
const formattedReply = `The sample project is ready for a local preview. The following checklist describes its configuration, build steps, and how to review the result while keeping the relevant files together.

**Project structure**
- The application reads its defaults from \`app.json\`. Local overrides belong in a separate configuration file so the same defaults can be reused by different previews.
- The build creates a bundle containing the application code and its assets. The generated output can be inspected independently of the source files.
- The preview server displays that output in a browser. Keep the browser open while checking the pages and their controls.

**Preparation steps**

1. **Check the configuration:** review the application name, entry point, and output directory. Confirm that the assets referenced by the example configuration exist in the project.

2. **Prepare the sample data:** use the local example dataset for the preview.
   - Keep the input files in the \`fixtures/\` directory with clear names.
   - Check that each example has enough content to exercise scrolling and navigation without depending on external services.

3. **Build and preview the project:** run the following commands from its directory.
   \`\`\`bash
   npm run check   # validate the source files
   npm run build   # generate the application bundle
   npm run preview # open the local preview
   \`\`\`
   Once the preview is ready, inspect the generated pages:
   - Open the overview and check the heading and description.
   - Navigate to a detailed page and inspect the formatted content.
   - Return to the overview and confirm that its controls still respond.

4. **Review the result:** compare the preview with the example data. Read the longer sections, inspect the code examples, and confirm that the final paragraph remains visible after the page finishes loading.

   Keep any notes next to the sample configuration so the next preview can repeat the same checks with a consistent dataset.

**Next steps:** the same project can be previewed with a second dataset by changing the local configuration and rebuilding. Each dataset should retain representative headings, lists, code examples, and long paragraphs.

All preparation steps are complete.`;

const bulletReply =
  Array.from(
    { length: 14 },
    (_, index) =>
      `- **Checkpoint ${index + 1}.** Inspect the example page and its supporting content. Read the description, follow the navigation controls, and review the detailed output. Keep the page open long enough to confirm that the final text is displayed consistently.`,
  ).join("\n\n") + "\n\nAll checks are complete.";

function Fixture() {
  const [pool] = useState(() => new TranscriptPool());
  const [id, select] = useState(0);
  const [blocks, setBlocks] = useState<Block[]>([...history, prompt]);
  const [busy, setBusy] = useState(true);
  useEffect(() => {
    Object.assign(window, {
      captureReaderAnchor,
      readerAnchorTop,
      selectSession: select,
      deliverReply: (text: string, complete = false) => {
        setBlocks([
          ...history,
          complete ? { ...prompt, durationMs: 2000 } : prompt,
          { id: "live-reply", role: "assistant", text, streaming: !complete },
        ]);
        setBusy(!complete);
      },
      sessionBurst: burst,
      formattedReply,
      bulletReply,
      transcriptPool: pool,
      transcriptPoolLimit: TRANSCRIPT_POOL_LIMIT,
    });
    const onKey = (event: KeyboardEvent) => {
      if (event.key >= "0" && event.key <= "9") select(Number(event.key));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [pool]);
  return (
    <>
      <section key={id} data-session={id} className="h-full">
        <PooledTranscript pool={pool} sessionId={`session-${id}`}>
          <AgentTranscript
            blocks={id === 0 ? blocks : history}
            busy={id === 0 && busy}
            visible
          />
        </PooledTranscript>
      </section>
      <TranscriptPoolOutlet pool={pool} />
    </>
  );
}
createRoot(document.getElementById("root")!).render(<Fixture />);
