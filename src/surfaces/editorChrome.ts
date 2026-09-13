import { syntaxHighlighting } from "@codemirror/language";
import type { Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { cspStyleNonce } from "../lib/csp";
import type { ColorScheme } from "../lib/appearance";
import { editorHighlightStyleFor } from "./editorLanguage";

export {
  editorHighlightStyleFor,
  languageForPath,
} from "./editorLanguage";

function editorThemeStyles(dark: boolean) {
  return EditorView.theme(
  {
    "&": {
      height: "100%",
      backgroundColor: "transparent",
      color: "var(--color-content)",
      fontSize: "var(--font-mono-size, 13px)",
      fontWeight: "var(--font-mono-weight, 400)",
      userSelect: "text",
    },
    "&.cm-focused": {
      outline: "none",
    },
    ".cm-scroller": {
      overflow: "auto",
      overscrollBehavior: "none",
      fontFamily: "var(--font-mono)",
      lineHeight: "1.6",
    },
    ".cm-content": {
      minHeight: "100%",
      padding: "8px 0 32px",
      caretColor: "var(--color-content)",
    },
    ".cm-line": {
      padding: "0 12px 0 6px",
    },
    ".cm-gutters": {
      backgroundColor: "transparent",
      color: "color-mix(in srgb, var(--color-content) 38%, transparent)",
      borderRight:
        "1px solid color-mix(in srgb, var(--color-content) 7%, transparent)",
      paddingLeft: "4px",
    },
    ".cm-lineNumbers .cm-gutterElement": {
      minWidth: "36px",
      padding: "0 8px 0 4px",
    },
    ".cm-foldGutter": {
      width: "12px",
    },
    ".cm-foldGutter .cm-gutterElement": {
      padding: "0 2px",
      cursor: "pointer",
      color: "color-mix(in srgb, var(--color-content) 45%, transparent)",
    },
    ".cm-foldPlaceholder": {
      color: "color-mix(in srgb, var(--color-content) 40%, transparent)",
    },
    ".cm-activeLine": {
      backgroundColor: "color-mix(in srgb, var(--color-content) 10%, transparent)",
    },
    ".cm-activeLineGutter": {
      backgroundColor: "color-mix(in srgb, var(--color-content) 10%, transparent)",
      color: "color-mix(in srgb, var(--color-content) 70%, transparent)",
    },
    ".cm-selectionBackground, &.cm-focused .cm-selectionBackground, ::selection":
      {
        backgroundColor:
          "color-mix(in srgb, var(--color-accent) 35%, transparent) !important",
      },
    ".cm-cursor, .cm-dropCursor": {
      borderLeftColor: "var(--color-content)",
    },
    ".cm-matchingBracket": {
      backgroundColor:
        "color-mix(in srgb, var(--color-content) 14%, transparent)",
      outline:
        "1px solid color-mix(in srgb, var(--color-content) 28%, transparent)",
    },
    ".cm-nonmatchingBracket": {
      backgroundColor: "color-mix(in srgb, #f87171 32%, transparent)",
      outline: "1px solid color-mix(in srgb, #f87171 55%, transparent)",
    },
    ".cm-panels, .cm-tooltip": {
      backgroundColor: "var(--color-background-base)",
      color: "var(--color-content)",
    },
    ".cm-panels": {
      borderColor: "color-mix(in srgb, var(--color-content) 10%, transparent)",
    },
    ".cm-tooltip": {
      border:
        "1px solid color-mix(in srgb, var(--color-content) 12%, transparent)",
      borderRadius: "6px",
      overflow: "hidden",
    },
    ".cm-tooltip.cm-tooltip-autocomplete": {
      backgroundColor: "var(--color-background-base)",
    },
    ".cm-tooltip-autocomplete > ul": {
      fontFamily: "var(--font-mono)",
      fontSize: "12px",
      maxHeight: "240px",
    },
    ".cm-tooltip-autocomplete > ul > li": {
      padding: "2px 8px",
    },
    ".cm-tooltip-autocomplete > ul > li[aria-selected]": {
      backgroundColor:
        "color-mix(in srgb, var(--color-accent) 25%, transparent)",
      color: "var(--color-content)",
    },
    ".cm-completionDetail": {
      color: "color-mix(in srgb, var(--color-content) 50%, transparent)",
      fontStyle: "normal",
      marginLeft: "8px",
    },
  }, { dark });
}

/**
 * The theme, plus whatever CSP nonce its style sheet needs. CodeMirror mounts
 * every rule it owns — base theme included — through style-mod at runtime, so a
 * style-src that rejects inline sheets leaves the editor completely unstyled.
 * See `cspStyleNonce` for why this is currently a no-op.
 */
export function editorThemeFor(scheme: ColorScheme): Extension {
  return [
    editorThemeStyles(scheme === "dark"),
    EditorView.cspNonce.of(cspStyleNonce()),
  ];
}

export function schemeExtensions(scheme: ColorScheme): Extension[] {
  return [
    editorThemeFor(scheme),
    syntaxHighlighting(editorHighlightStyleFor(scheme)),
  ];
}

