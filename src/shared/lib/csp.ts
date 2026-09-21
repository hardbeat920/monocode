/**
 * Reads the CSP nonce Tauri stamps onto the `<style>` tags in index.html.
 *
 * Tauri normally rewrites the configured `style-src` to carry a generated nonce
 * (tauri-utils `inject_nonce_token` / `set_csp`). A nonce in the directive makes
 * the browser ignore `'unsafe-inline'`, which silently blocks every style sheet
 * mounted at runtime — CodeMirror's style-mod included. That is why the packaged
 * app once rendered the editor completely unstyled.
 *
 * `dangerousDisableAssetCspModification: ["style-src"]` in tauri.conf.json turns
 * that rewriting off, so today no nonce is injected and this returns "" — which
 * is exactly what CodeMirror defaults to. It is kept so the editor still renders
 * if style-src nonce hardening is ever turned back on.
 */
const styleNonce =
  document.querySelector<HTMLStyleElement>("style[nonce]")?.nonce ?? "";

export function cspStyleNonce(): string {
  return styleNonce;
}
