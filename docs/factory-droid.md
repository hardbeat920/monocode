# Factory Droid

MonoCode runs Factory Droid through its ACP command, `droid exec --output-format acp`.

Install the [Factory CLI](https://docs.factory.ai/cli/getting-started/overview), then run `droid` in Terminal and sign in with `/login`. You can also provide `FACTORY_API_KEY` to the process that starts MonoCode or the remote host.

Choose Factory Droid in the model picker. MonoCode discovers the model list and each model's reasoning choices through a temporary ACP session. The probe switches models without sending a prompt. If discovery fails or returns incomplete settings, MonoCode preserves saved model IDs and reasoning choices until it receives a complete catalog.

The provider settings page supports a configured Droid executable. A changed executable takes effect after restarting MonoCode.

## Sessions and approvals

Droid supports text, image and file attachments. MonoCode displays streamed messages, tool calls, plans and subagent activity. Follow-up messages wait until the current turn ends. Saved sessions resume through ACP when Droid supports loading the saved session ID.

Runtime settings map to Droid's modes as follows.

| MonoCode setting  | Droid mode    |
| ----------------- | ------------- |
| Supervised        | `normal`      |
| Auto-accept edits | `auto-low`    |
| Auto              | `auto-medium` |
| Full access       | `auto-high`   |
| Planning          | `spec`        |

MonoCode answers permission requests with the option IDs Droid supplies. Supervised mode asks for a decision. Auto-accept edits permits read, search and edit requests, and asks before other tool kinds. Cancelling a turn cancels pending permission requests and stops the child process. Model, effort and mode changes must succeed before MonoCode sends the next prompt.

## Skills

MonoCode discovers project skills in `.factory/skills` and user skills in `~/.factory/skills`. New skills created through MonoCode use `.agents/skills` so providers can share one copy.

## Remote hosts

Install and sign in to Droid on the machine running `monocode-host`. That host owns the child process, model catalog and session state. Droid sessions use the existing remote session and approval controls.

## Local usage

The footer displays Factory's standard five-hour, weekly and monthly usage windows when the local CLI has readable credentials. The Tauri backend decrypts the CLI's stored credentials and calls Factory's billing endpoint. It does not send credentials to the webview, refresh tokens or write credential files.

`FACTORY_HOME_OVERRIDE` selects the home directory containing `.factory`. The newest credential file is authoritative. MonoCode reports unavailable usage if it cannot read that file, rather than trying an older account. macOS uses Keychain, Linux keyring access requires `secret-tool`, and Windows uses Credential Manager. The CLI's file-backed encryption key is also supported.

Usage is unavailable for remote sessions and configured executables whose account home MonoCode cannot determine. These sessions never use the desktop's cached usage. API-key-only sessions without stored CLI credentials also have no usage snapshot.
