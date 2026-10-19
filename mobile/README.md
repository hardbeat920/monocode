# Monocode mobile

The Expo app lives in this directory and has its own npm dependencies and lockfile. The app pairs with the desktop over QUIC and shows its real workspace. Use a Node.js LTS release for development.

## Run locally

From the repository root:

```bash
cd mobile
npm ci
npm run ios
```

`npm run ios` first builds the Rust connection library, then creates and installs the development build in an iOS simulator, then starts Metro. It requires Xcode and an installed iOS Simulator runtime (Xcode > Settings > Components). For Android, install Android Studio and start an emulator, install an Android NDK, set `ANDROID_NDK_HOME`, then use `npm run android`. Rust and rustup must be installed for both platforms. Use `npm run ios -- --device` or `npm run android -- --device` to build for a connected physical device. A physical iPhone needs Developer Mode enabled and a unique `ios.bundleIdentifier` in `app.json`.

After the first build, use `npm start` for everyday TypeScript and JavaScript changes. Press `i` or `a` in the Expo terminal to open the installed development build in a simulator or emulator. Rebuild after adding native modules, changing `app.json`, or upgrading Expo. Regenerate native folders with `npx expo prebuild --clean` before rebuilding in those cases. The generated `ios/` and `android/` folders are ignored by Git.

The routes are in `src/app/`:

- `_layout.tsx` gates the workspace behind pairing and puts it inside `SimpleDrawer` (`src/ui/SimpleDrawer.tsx`), a gesture-driven side drawer. The drawer content is `src/features/workspace/WorkspaceDrawer.tsx`, which lists monos, projects and recent sessions, with search.
- `project/[projectId]/index.tsx` is the new session screen for a project.
- `project/[projectId]/session/[sessionId].tsx` is a session transcript.
- `mono/[monoId].tsx` is a mono's conversation.

## Pair with desktop

Run the desktop app from the repository root, open **Settings → Mobile**, enable mobile access and choose **Pair device**. Scan the code in the mobile app, then choose the projects and monos to include. You can select nothing and choose later. **Mobile Settings → Projects and monos** changes the selection at any time; choices are remembered per desktop, including after reconnecting.

Pairing codes expire after 15 minutes and work once. The shared Rust module uses QUIC and mutually authenticated TLS 1.3, with the desktop identity pinned by the code. Long-lived private keys are stored in the desktop OS keyring and the phone’s SecureStore. Desktop settings list paired devices and let you revoke them. Selection controls what appears on the phone; a paired device has workspace access.

Remote connections go directly to the desktop. Automatic NAT-PMP/UPnP router mapping is enabled when mobile access is first enabled. A restrictive router or carrier NAT can require a reachable VPN address or manual UDP forwarding to port **38416**. Configure the remote address before generating a code. The desktop must remain awake and running.

Workspace lists refresh while the phone is active. Opening a conversation reads its latest 200 user/assistant blocks from the desktop, with active conversation updates, text sending, starting sessions and stopping turns. The changes view reads real Git changes on demand. Agent approvals and questions still need attention on the desktop. The app retains the workspace list for offline navigation and keeps failed-send drafts; it does not queue commands offline.

The transport is an Expo local native module in `modules/monocode-link`, built by `scripts/build-native-link.mjs`. **Expo Go cannot load it**; rebuild the development app after changing native code. `npm run native:ios` and `npm run native:android` build the library separately. Generated native libraries and platform folders are ignored by Git.

## Switch provider accounts

Tap the model in the composer, then **Account**, to choose a Claude Code or Codex profile saved on your desktop. Each provider keeps its own selection. Choosing another account from an existing chat opens a new conversation with the same model and permissions; the original stays in history. Add accounts and complete sign-in in desktop **Settings → Accounts**.

The account card also shows the desktop footer's usage windows, percentages and reset times. Tap **Refresh** or pull down in the model picker to reload usage for the provider's accounts.

## EAS development builds

Run EAS commands from `mobile/`. The build input must include the root Cargo workspace and its path dependencies. The `eas-build-pre-install` hook installs the pinned Rust toolchain and builds the native transport before CocoaPods/Gradle runs.

The `development` profile builds for Android or physical iOS devices; `development-simulator` builds for the iOS simulator. EAS requires an Expo account, and physical iOS distribution requires Apple signing credentials.

```bash
npx eas-cli@latest login
npx eas-cli@latest build --platform ios --profile development-simulator
# or: npx eas-cli@latest build --platform android --profile development
```

Install the completed development build, then run `npm start` to connect it to Metro.

## Check the project

```bash
npx tsc --noEmit
npm run lint
```

The desktop app at the repository root remains a separate npm project.
