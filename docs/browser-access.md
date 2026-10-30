# Browser access

Agents can open and use their own tabs in a browser. They never see or touch your own tabs.

## Turn it on

Open Settings → Browser and switch on **Let agents use my browser**. MonoCode remembers the switch across restarts. Nothing starts or connects until an agent first needs the browser.

## Pick a browser

**Browser** in Settings → Browser has three choices.

- **MonoCode browser** (the default): MonoCode starts an installed Chrome, Edge, Brave, Vivaldi, Opera or Arc (macOS only) with its own profile. Nothing to set up, and your own browser does not have to run. **Which browser to start** picks one; Automatic starts Chrome, or else the first of the others that is installed. **Open browser** shows its window so you can sign in to the sites agents need; the profile keeps those sign-ins. Switch off **Show browser window** to run it without a window. When it closes, it starts again the next time an agent needs it.
- **Private MonoCode browser**: the same with a fresh profile every time it starts, deleted when it closes. No sign-ins, cookies or history are kept.
- **Your own browser**: MonoCode connects to the Chrome, Edge, Brave, Vivaldi, Opera or Arc you run, signed in as you. Once, turn on remote debugging at `chrome://inspect/#remote-debugging` (Chrome, Vivaldi, Opera, Arc), `brave://inspect/#remote-debugging` (Brave) or `edge://inspect` (Edge). The browser asks you to allow every new connection. While MonoCode is connected, the browser shows a bar saying it is controlled by automated software; MonoCode cannot hide it.

The MonoCode browser starts with switches of its own. To try others without a new build, set `MONOCODE_BROWSER_ARGS` in MonoCode's environment to the switches, separated by spaces (for instance `--enable-gpu --use-angle=d3d11`); they apply the next time the browser starts.

## Give a thread access

**Thread access** in Settings → Browser decides how a thread gets the browser.

- **Ask in the thread** (the default): the first time an agent needs the browser, the thread shows "This agent wants to use the browser." above the composer. **Allow for this thread** gives the thread access until you take it away. **Not now** tells the agent to go on without the browser; it does not ask again until your next message. The agent waits about 45 seconds for your answer. On a paired phone the same request shows in the thread's composer, with **Allow** and **Not now**, and as a notification while the app is open in the background; the browser itself still runs on the desktop.
- **Always on**: every thread gets access without asking.

A thread set to **Full access** in the composer's access picker gets access without asking in both modes.

`/browser` at the start of a message works in both modes. Alone it switches access on or off for that thread; followed by a prompt, such as `/browser check the pricing page on example.com`, it turns access on and sends the prompt. Right-click a thread in the sidebar for the same **Browser access** switch.

A thread with access shows a globe next to the context meter: steady while it has access, pulsing while an action runs, amber when one of its tabs waits on a dialog. Click it for the thread's tabs and **Turn off**.

## Take access away

Turn a thread off with `/browser`, the sidebar switch, the globe's **Turn off** or **Revoke** in Settings → Browser. The thread then asks again the next time its agent needs the browser, even with Always on or Full access, until you allow it. This holds in every MonoCode window until you quit MonoCode or close the thread's window. Switching off **Let agents use my browser** takes access from every thread.

## Who never gets it on their own

Thread access, Full access and `/browser` apply to messages you send in a regular thread. Orchestration runs and their workers, sessions another agent starts through the app, and follow-ups an agent sends to a session use the browser only when the thread already has access.

The exception is a Mono with browser access. A session the Mono starts gets the Mono's access, and so does a session it started when the Mono hands it more work. Such a session never asks you: if the Mono has no access at that moment, the session works without the browser. A session you turned off stays off. Turning the Mono off later does not take access from sessions it already passed it to.

## Agent tabs

In your own browser, agent tabs open in the background of your current window with `◆ MonoCode` in front of their title, or each in a window of its own with **Agent tabs in a separate window**. Settings → Browser lists every agent tab; the thread's globe lists that thread's.

- **Take back**: the agent loses the tab; it stays open for you.
- **Move to…** (Settings only): hand the tab to another thread that has access.
- **Close**: close it in the browser.
- **Point at an element** (the cursor button, in the globe only): the browser highlights what your pointer is over; click the element you mean. Your message gets a line naming it, with a picture, and the agent's next snapshot of that tab marks it. Click the button again or **Cancel** to stop. It needs a visible browser window.

Turning a thread or the main switch off, or a dropped connection, leaves the agent tabs open.

While you watch an agent tab, a MonoCode cursor labelled `◆ MonoCode` shows where the agent points. It never takes clicks, the page cannot see it, and screenshots leave it out. In a background tab or with reduced motion on, it jumps instead of gliding.

## Hand a tab to a thread

A tab you open yourself belongs to no thread. In Settings → Browser, click **Show my tabs** under **Give a tab to a thread**, pick a thread next to a tab and click **Give**; in a thread's globe, use **Add a tab from the browser…**. The thread gets browser access with the tab. This needs MonoCode connected to the browser.

- **Your own browser**: handing a tab over is the only way a thread gets one of your tabs, and it can use that tab and none of your others.
- **MonoCode browser**: tabs you open there are free, and any thread with access can also take one by itself.

## Each branch's app

When a thread's worktree runs a dev server (`npm run dev`, `vite`, `next dev`, `python -m http.server` and the like), MonoCode finds the port it listens on and keeps it with that worktree, so threads on different branches each check their own app.

- While the thread has the browser, every turn tells the agent the address its worktree serves, such as `http://localhost:5174`. Listing its tabs gives the same address, so a server started during the turn is found too.
- The thread's globe lists **This branch's app**. **Open in a tab** opens it in a tab of that thread, which gets browser access with it.

A server counts as the worktree's when MonoCode started it, through the thread's agent or a MonoCode terminal in that worktree. On macOS and Linux, a server started elsewhere also counts when it runs inside the worktree's folder. Only ports that answer web requests on `localhost` are listed.

## Console and network

Agents read the console of their own tabs and see failed requests: console messages, uncaught errors with file and line, browser warnings, and requests that failed or returned an error status. After every action the agent hears whether the page logged new errors or had failed requests.

- On a local server (such as `localhost:5173`), MonoCode listens from the moment the page opens and shows the start of an error response.
- On other sites it listens to the console only once the agent asks, because some sites treat that as a bot. Failed requests are recorded everywhere; other sites' addresses are shown without their query.

This needs no extra permission and never covers your own tabs. None of it is kept in the activity list or after MonoCode closes.

## What the switch covers

Turning browser access off stops every MonoCode browser action before it reaches the browser. An action already sent can still finish, and nothing a site already did is undone. The switch covers MonoCode's own browser tools only; other programs, extensions or tools you gave an agent yourself reach the browser their own way.

## Activity

Settings → Browser lists the last 50 browser actions with their thread, outcome and duration, kept for 30 days. Typed text and page content are never stored, and addresses are kept without their query.

Return to the [README](../README.md) for a quick start.
