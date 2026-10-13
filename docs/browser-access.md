# Browser access with /browser

Agents can open and use their own tabs in the Chrome or Edge you already run, signed in as you. They never see or touch your own tabs.

## Turn it on

1. Open Settings → Browser and switch on **Let agents use my browser**. The switch starts off every time MonoCode starts.
2. If MonoCode asks for it, open `chrome://inspect/#remote-debugging` (Chrome) or `edge://inspect` (Edge) and turn on remote debugging.
3. Click **Connect**. The browser asks whether to allow the connection; choose Allow. Every new connection asks again.

While MonoCode is connected, the browser shows a bar saying it is controlled by automated software. The browser draws that bar and MonoCode cannot hide it.

## Give a thread access

Type `/browser` at the start of a composer message.

- `/browser` alone switches access on or off for that thread.
- `/browser` followed by a prompt turns access on and sends the prompt, for example `/browser check the pricing page on example.com`.
- The thread's menu in the sidebar has the same **Browser access** switch.

Access lasts until you turn it off, revoke it in Settings, switch off browser access for all agents, or restart MonoCode. Another thread or an agent cannot turn it on for a thread. Orchestration workers do not get browser access.

A thread with access shows a globe next to the context meter: steady while it has access, pulsing while an action runs, and amber when one of its tabs waits on a dialog. Click it to see the thread's tabs.

## Agent tabs

Agent tabs open in the background of your current window, with `◆ MonoCode` in front of their title. Settings → Browser can open them in separate windows instead. For every tab you can:

- **Take back**: the agent loses the tab; it stays open for you.
- **Move to…**: hand the tab to another thread that has access.
- **Close**: close it in the browser.

Revoking a thread, switching browser access off or a dropped connection leaves the tabs open.

## What the switch covers

Turning browser access off stops every MonoCode browser action before it reaches the browser. An action already sent can still finish, and nothing a site already did is undone. The switch covers MonoCode's own browser tools only; other programs, extensions or tools you gave an agent yourself reach the browser their own way.

## Activity

Settings → Browser lists the last 50 browser actions, with their thread, outcome and duration, kept for 30 days. Typed text and page content are never stored, and addresses are kept without their query.

Return to the [README](../README.md) for a quick start.
