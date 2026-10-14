# Browser access

Agents can open and use their own tabs in a browser. They never see or touch your own tabs.

## Turn it on

Open Settings → Browser and switch on **Let agents use my browser**. MonoCode remembers the switch across restarts. Nothing starts or connects when MonoCode starts; the browser starts, or MonoCode connects to it, when an agent first needs it.

## Pick a browser

**Browser** in Settings → Browser has three choices.

- **MonoCode browser** (the default): MonoCode starts a browser you have installed, with its own profile in MonoCode's app data. It works with Chrome, Edge, Brave, Vivaldi, Opera and Arc (Arc on macOS only). It needs no setup, and your own browser does not have to run. **Which browser to start** picks one of them; Automatic starts Chrome, or else the first of Edge, Brave, Vivaldi, Opera and Arc that is installed. Click **Open browser** to bring up its window and sign in to the sites your agents need. The profile keeps those sign-ins for next time. Switch off **Show browser window** to run it without a window.
- **Private MonoCode browser**: the MonoCode browser with a fresh profile every time it starts. MonoCode deletes the profile when the browser closes, so nothing is kept: no sign-ins, cookies or history. Use it when agents should see sites as a new visitor.
- **Your own browser**: MonoCode connects to the Chrome, Edge, Brave, Vivaldi, Opera or Arc you already run, signed in as you. The first time, open `chrome://inspect/#remote-debugging` (Chrome, Vivaldi, Opera, Arc), `brave://inspect/#remote-debugging` (Brave) or `edge://inspect` (Edge) and turn on remote debugging. When MonoCode connects, the browser asks whether to allow the connection; choose Allow. Every new connection asks again. While MonoCode is connected, the browser shows a bar saying it is controlled by automated software. The browser draws that bar and MonoCode cannot hide it.

## Give a thread access

**Thread access** in Settings → Browser decides how a thread gets the browser.

- **Ask in the thread** (the default): the first time an agent needs the browser, its thread shows "This agent wants to use the browser." above the composer. **Allow for this thread** gives the thread access, and it keeps it. **Not now** tells the agent to go on without the browser; it does not ask again until your next message. The agent waits for your answer for about 45 seconds. With the MonoCode browser, the browser already starts while you decide.
- **Always on**: every thread can use the browser without asking.

A thread you set to **Full access** in the composer's access picker gets the browser without asking, in both modes.

`/browser` at the start of a composer message works in both modes.

- `/browser` alone switches access on or off for that thread.
- `/browser` followed by a prompt turns access on and sends the prompt, for example `/browser check the pricing page on example.com`.
- The thread's menu in the sidebar has the same **Browser access** switch.

A thread you turn off asks again the next time its agent needs the browser, even with Always on or Full access. Access lasts until you turn it off, revoke it in Settings, switch off browser access for all agents, or restart MonoCode. Another thread or an agent cannot turn it on for a thread. Orchestration runs and their workers, and sessions another agent starts through the app, never get the browser on their own.

A thread with access shows a globe next to the context meter: steady while it has access, pulsing while an action runs, and amber when one of its tabs waits on a dialog. Click it to see the thread's tabs.

## Agent tabs

In your own browser, agent tabs open in the background of your current window, with `◆ MonoCode` in front of their title. Settings → Browser can open them in separate windows instead. For every tab you can:

- **Take back**: the agent loses the tab; it stays open for you.
- **Move to…**: hand the tab to another thread that has access.
- **Close**: close it in the browser.
- **Point at an element** (the cursor button, in the thread's globe only): the browser comes to the front and highlights what your pointer is over. Click the element you mean. Your message gets a line naming it for the agent, with a picture of it, and the agent's next snapshot of that tab marks it. Click the button again or **Cancel** to stop. It needs a visible browser window.

Revoking a thread, switching browser access off or a dropped connection leaves the tabs open.

While you watch an agent tab, a dark MonoCode cursor with a `◆ MonoCode` label shows where the agent points: it glides to what the agent clicks, hovers or types into, and pulses on a click. It fades after a few seconds without actions and goes away when you take the tab back. It never takes clicks, the page cannot see inside it, and the agent's screenshots leave it out. In a background tab, or with reduced motion turned on in your system, it jumps instead of gliding, so the agent is not slowed down.

## Console and network

Agents can read the console of their own tabs and see which requests failed, so they can debug a web app they work on without asking you to open DevTools. They see console messages, uncaught JavaScript errors with the file and line, the browser's own warnings, and requests that came back with an error status or failed. After every action, the agent hears whether the page logged new errors or had failed requests since its last look.

- On a local server (an address like `localhost:5173`), MonoCode listens to the console from the moment the page opens, and shows the agent the start of a local server's error response.
- On other sites it listens to the console only once the agent asks for it, because some sites notice that and treat the browser as a bot. Failed requests are recorded on every site; the agent sees other sites' addresses without their query.

This needs no extra permission: a thread with browser access has it for its own tabs, and never for yours. None of it is stored in the activity list or after MonoCode closes.

## Hand a tab to a thread

A tab you open yourself belongs to no thread. To let a thread work in it, open Settings → Browser and click **Show my tabs** under **Give a tab to a thread**. The list shows the web pages in the browser that no thread holds. Pick a thread next to a tab and click **Give**. The thread gets browser access with the tab. The control shows while browser access is on and MonoCode is connected to the browser.

The globe in a thread with access has the same control for that thread: click it, then **Add a tab from the browser…**, and click **Add** next to a tab.

- **Your own browser**: agents never see your own tabs, so handing one over is the only way a thread gets one of them. The agent can use that tab and none of your others.
- **MonoCode browser**: tabs you open there are free, and any thread's agent with access can take one by itself. You can also give one to a thread yourself.

**Take back** returns a tab to you.

## What the switch covers

Turning browser access off stops every MonoCode browser action before it reaches the browser. An action already sent can still finish, and nothing a site already did is undone. The switch covers MonoCode's own browser tools only; other programs, extensions or tools you gave an agent yourself reach the browser their own way.

## Activity

Settings → Browser lists the last 50 browser actions, with their thread, outcome and duration, kept for 30 days. Typed text and page content are never stored, and addresses are kept without their query.

Return to the [README](../README.md) for a quick start.
