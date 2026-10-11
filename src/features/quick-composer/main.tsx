import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { IS_MAC } from "../../platform/tauri/platform";
import {
  applyAccentColor,
  applyThemeDarkLightness,
  applyThemePreference,
  applyThemeTint,
  loadAccentColor,
  loadThemeDarkLightness,
  loadThemeHue,
  loadThemePreference,
  loadThemeSaturation,
} from "../settings/model/appearance";
import { QuickGitPopup } from "./ui/QuickGitPopup";
import { QuickComposer } from "./ui/QuickComposer";
import "../../styles/index.css";

/**
 * Only the theme, not the workspace's glass, backgrounds, or scale: the panel
 * draws its own card on a clear window. Re-applied on every show because the
 * theme may have changed in a workspace window since.
 */
function applyAppearance() {
  document.documentElement.classList.toggle("is-mac", IS_MAC);
  applyAccentColor(loadAccentColor());
  applyThemeTint(loadThemeHue(), loadThemeSaturation());
  applyThemeDarkLightness(loadThemeDarkLightness());
  const preference = loadThemePreference();
  const scheme = applyThemePreference(preference);
  // The native blur follows the window's appearance, not the page's, so
  // match it to the app theme rather than the system one. The theme is
  // app-wide on macOS, so "system" is left cleared: pinning the resolved
  // scheme would hold every window at the OS appearance of this moment.
  if (preference === "system") return;
  void getCurrentWindow()
    .setTheme(scheme)
    .catch(() => undefined);
}

applyAppearance();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {new URLSearchParams(window.location.search).get("popup") === "git" ? (
      <QuickGitPopup onShown={applyAppearance} />
    ) : (
      <QuickComposer onShown={applyAppearance} />
    )}
  </React.StrictMode>,
);
