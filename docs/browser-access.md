# Browser access

Agents can open and use their own tabs in a browser. They never see or touch your own tabs.

## Turn it on

Open Settings → Browser and switch on **Let agents use my browser**. MonoCode remembers the switch across restarts. Nothing starts or connects when MonoCode starts; the browser starts, or MonoCode connects to it, when an agent first needs it.

## Pick a browser

**Browser** in Settings → Browser has two choices.

- **MonoCode browser** (the default): MonoCode starts the Chrome or Edge you have installed, with its own profile in MonoCode's app data. It needs no setup, and your own Chrome does not have to run. **Which browser to start** picks Chrome or Edge; Automatic starts Chrome, or Edge when Chrome is not installed. Click **Open browser** to bring up its window and sign in to the sites your agents need. The profile keeps those sign-ins for next time. Switch off **Show browser window** to run it without a window.
- **Your Chrome or Edge**: MonoCode connects to the browser you already run, signed in as you. The first time, open `chrome://inspect/#remote-debugging` (Chrome) or `edge://inspect` (Edge) and turn on remote debugging. When MonoCode connects, the browser asks whether to allow the connection; choose Allow. Every new connection asks again. While MonoCode is connected, the browser shows a bar saying it is controlled by automated software. The browser draws that bar and MonoCode cannot hide it.

## Give a thread access

**Thread access** in Settings → Browser decides how a thread gets the browser.

- **Ask in the thread** (the default): the first time an agent needs the browser, its thread shows "This agent wants to use the browser." above the composer. **Allow for this thread** gives the thread access, and it keeps it. **Not now** tells the agent to go on without the browser; it does not ask again until your next message. The agent waits for your answer for about 50 seconds.
- **Always on**: every thread can use the browser without asking.

`/browser` at the start of a composer message works in both modes.

- `/browser` alone switches access on or off for that thread.
- `/browser` followed by a prompt turns access on and sends the prompt, for example `/browser check the pricing page on example.com`.
- The thread's menu in the sidebar has the same **Browser access** switch.

A thread you turn off asks again the next time its agent needs the browser, even with Always on. Access lasts until you turn it off, revoke it in Settings, switch off browser access for all agents, or restart MonoCode. Another thread or an agent cannot turn it on for a thread. Orchestration runs and their workers, and sessions another agent starts through the app, never get the browser on their own.

A thread with access shows a globe next to the context meter: steady while it has access, pulsing while an action runs, and amber when one of its tabs waits on a dialog. Click it to see the thread's tabs.

## Agent tabs

In your own Chrome or Edge, agent tabs open in the background of your current window, with `◆ MonoCode` in front of their title. Settings → Browser can open them in separate windows instead. For every tab you can:

- **Take back**: the agent loses the tab; it stays open for you.
- **Move to…**: hand the tab to another thread that has access.
- **Close**: close it in the browser.

Revoking a thread, switching browser access off or a dropped connection leaves the tabs open.

## What the switch covers

Turning browser access off stops every MonoCode browser action before it reaches the browser. An action already sent can still finish, and nothing a site already did is undone. The switch covers MonoCode's own browser tools only; other programs, extensions or tools you gave an agent yourself reach the browser their own way.

## Activity

Settings → Browser lists the last 50 browser actions, with their thread, outcome and duration, kept for 30 days. Typed text and page content are never stored, and addresses are kept without their query.

Return to the [README](../README.md) for a quick start.
