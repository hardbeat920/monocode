import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  applyAccentColor,
  activateWindowAppearance,
  applyBodyGlass,
  applySidebarBlur,
  applySidebarOpacity,
  applyThemeDarkLightness,
  applyThemePreference,
  applyThemeTint,
  loadAccentColor,
  loadBodyGlass,
  loadSidebarBlur,
  loadSidebarOpacity,
  loadThemeDarkLightness,
  loadThemeHue,
  loadThemePreference,
  loadThemeSaturation,
  watchSystemColorScheme,
} from "../settings/model/appearance";
import { FloatingMonoChat } from "./ui/FloatingMonoChat";
import { homeDir } from "../../platform/tauri/fs";
import { setHomeDir } from "../../shared/lib/paths";
import "../../styles/index.css";

let appearanceKey = "";
function applyFloatingAppearance() {
  const nextKey = JSON.stringify([
    loadAccentColor(),
    loadThemeHue(),
    loadThemeSaturation(),
    loadThemeDarkLightness(),
    loadThemePreference(),
    loadSidebarOpacity(),
    loadSidebarBlur(),
    loadBodyGlass(),
  ]);
  if (nextKey === appearanceKey) return;
  appearanceKey = nextKey;
  document.documentElement.classList.add("is-mac");
  applyAccentColor(loadAccentColor());
  applyThemeTint(loadThemeHue(), loadThemeSaturation());
  applyThemeDarkLightness(loadThemeDarkLightness());
  applySidebarOpacity(loadSidebarOpacity());
  applySidebarBlur(loadSidebarBlur());
  applyBodyGlass(loadBodyGlass());
  const scheme = applyThemePreference(loadThemePreference());
  activateWindowAppearance();
  void getCurrentWindow()
    .setTheme(scheme)
    .catch(() => undefined);
}

applyFloatingAppearance();
watchSystemColorScheme();
window.addEventListener("storage", applyFloatingAppearance);
void homeDir()
  .then(setHomeDir)
  .catch(() => undefined);
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <FloatingMonoChat onShown={applyFloatingAppearance} />
  </React.StrictMode>,
);
