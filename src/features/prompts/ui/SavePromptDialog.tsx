import { useEffect, useState, type ReactNode } from "react";
import { Modal } from "../../../shared/ui/Modal";
import {
  promptTitleFrom,
  SAVE_PROMPT_REQUEST_EVENT,
  saveSavedPrompt,
} from "../model/savedPrompts";
import { SavedPromptForm } from "./SavedPromptForm";

/**
 * Mounted once per window. Opens a small Save prompt dialog whenever
 * `requestSavePrompt(text)` runs (e.g. "Add to Prompts" on selected text),
 * so the text can be named and trimmed before it is saved.
 */
export function SavePromptDialogHost(): ReactNode {
  const [request, setRequest] = useState<{ id: number; text: string } | null>(
    null,
  );

  useEffect(() => {
    const onRequest = (event: Event): void => {
      const detail = (event as CustomEvent<{ text?: unknown }>).detail;
      if (typeof detail?.text !== "string") return;
      setRequest({ id: Date.now(), text: detail.text });
    };
    window.addEventListener(SAVE_PROMPT_REQUEST_EVENT, onRequest);
    return () =>
      window.removeEventListener(SAVE_PROMPT_REQUEST_EVENT, onRequest);
  }, []);

  if (!request) return null;
  const close = (): void => setRequest(null);
  return (
    <Modal
      title="Save to Prompts"
      description="Type ! in any composer to insert it."
      onClose={close}
    >
      <div className="p-4">
        <SavedPromptForm
          key={request.id}
          initial={{
            title: promptTitleFrom(request.text),
            body: request.text.trim(),
            pinned: false,
          }}
          onCancel={close}
          onSave={async (draft) => {
            await saveSavedPrompt({ id: crypto.randomUUID(), ...draft });
            close();
          }}
        />
      </div>
    </Modal>
  );
}
