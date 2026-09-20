import { autocompletion } from "@codemirror/autocomplete";
import type { Extension } from "@codemirror/state";

export const editorAutocomplete: Extension = autocompletion({
  activateOnTyping: true,
  selectOnOpen: true,
  defaultKeymap: true,
});
