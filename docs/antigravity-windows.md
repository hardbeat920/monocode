# Antigravity on Windows

MonoCode's Windows desktop build connects to Antigravity directly. You do not need to install OpenCode, `agy`, or a Node.js runtime for this integration. macOS and Linux continue to use the Antigravity CLI and ACP adapter.

## Connect an account

1. Open MonoCode Settings and find Antigravity.
2. Select **Connect Google**. MonoCode opens the Google sign-in page in your browser only after this action.
3. Complete sign-in and return to MonoCode. The account email appears in Settings and compatible Antigravity models become available in the model selector.

The Windows integration currently stores one Google account for the current Windows user. You can refresh the model catalog or disconnect the account from the same Settings control. Connecting again after disconnecting replaces the saved account.

OAuth uses a loopback callback and PKCE. Access and refresh tokens stay in the native Rust backend and are protected with Windows DPAPI for the current user. Tokens are not sent to the frontend or written to conversation history or logs.

## Models

The native catalog currently includes Gemini 3.1 Pro, Gemini 3.5/3.6/3.7/3.8 Flash, Claude Sonnet 4.6 Thinking, Claude Opus 4.6 Thinking, Gemini 3.1 Flash Image, and GPT-OSS 120B Medium. Gemini thinking variants are selectable where the model supports them. The catalog is scoped to the models and request formats implemented by the native backend; account eligibility, service availability, and quota are determined by Google.

## Troubleshooting

- If **Connect Google** is unavailable, restart the Windows desktop app and confirm that the native backend is available.
- If sign-in is cancelled or the callback expires, select **Connect Google** again.
- If a saved account has expired credentials, disconnect it and sign in again.
- If model discovery reports an account or quota error, check the Google account and service access, then use **Refresh models** after access is restored.
- If the callback port is already in use, close the application using it and retry sign-in.

The current integration is for the local Windows desktop runtime. Remote sessions and macOS/Linux continue through their existing Antigravity transport.
