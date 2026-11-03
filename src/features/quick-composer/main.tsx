import React from "react";
import ReactDOM from "react-dom/client";
import { IS_MAC } from "../../platform/tauri/platform";
import { installNativeContextMenuGuard } from "../../shared/lib/nativeContextMenu";
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
  pinPanelWindowTheme,
  watchSystemColorScheme,
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
  pinPanelWindowTheme(preference, applyThemePreference(preference));
}

applyAppearance();
watchSystemColorScheme();
installNativeContextMenuGuard();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {new URLSearchParams(window.location.search).get("popup") === "git" ? (
      <QuickGitPopup onShown={applyAppearance} />
    ) : (
      <QuickComposer onShown={applyAppearance} />
    )}
  </React.StrictMode>,
);
