/// <reference types="vite/client" />

declare module "tauri-native-core" {
  export * from "@tauri-apps/api/core";
}

declare module "tauri-native-event" {
  export * from "@tauri-apps/api/event";
}

declare module "tauri-native-window" {
  export * from "@tauri-apps/api/window";
}

declare module "tauri-native-webview" {
  export * from "@tauri-apps/api/webview";
}

declare module "tauri-native-app" {
  export * from "@tauri-apps/api/app";
}

declare module "tauri-native-dialog" {
  export * from "@tauri-apps/plugin-dialog";
}
