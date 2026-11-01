# Hotspot join: reliability audit

Keep-awake can move the Mac onto a phone hotspot when Wi-Fi drops while agents
work, then move it back. Users called it "really bad, unpredictable and not
reliable". This file records what was wrong, what changed, and what still
needs a real phone to check.

Line numbers refer to `snapshot/inbox-base`, before the fixes.

Tags: **[code]** = verified by reading the code or by tests.
**[mac]** = observed on this machine (macOS 27.0.1, build 26A434) with
read-only commands. **[general]** = general knowledge of macOS and iOS
behavior, not verified here.

## What macOS 27 tells an app without Location access

Read-only checks on macOS 27.0.1 while en0 was connected:

- `networksetup -getairportnetwork en0` prints "You are not associated with an
  AirPort network." even though en0 has a lease and is the default route.
  **[mac]**
- `ipconfig getsummary en0` shows `SSID : <redacted>` and
  `BSSID : <redacted>`. **[mac]**
- `system_profiler SPAirPortDataType -json` reports
  `spairport_status_connected` but the network `_name` is `<redacted>`, and the
  call takes about 6.6 s. **[mac]**
- `ipconfig getifaddr en0` and `ipconfig getoption en0 router` still return
  the address and the router, with no extra permission. **[mac]**
- An iPhone Personal Hotspot always hands out 172.20.10.0/28, with router
  172.20.10.1 and mask 255.255.255.240. **[mac]** (observed) **[general]**
  (documented)

So the Mac can't check by name which network it joined. It can tell that the
Wi-Fi link changed (new lease or router), and it can recognize an iPhone
hotspot by its subnet.

## Defects, most likely cause of the reported symptoms first

1. **Idle iPhone hotspots can't be found, and the code retried blindly.**
   `hotspot.rs:202-212`, `RETRY` at `hotspot.rs:12`. An iPhone stops
   broadcasting its hotspot about 90 s after the last device leaves, unless the
   Personal Hotspot screen is open. **[general]** Instant Hotspot wakes it over
   Bluetooth, but only from the Wi-Fi menu or macOS's own auto-join, through
   private Continuity APIs. **[general]** `networksetup -setairportnetwork`
   then fails with "Could not find network". *Scenario:* "Test now" works
   because the user has the phone's Hotspot screen open. With the phone in a
   pocket, every join fails, once a minute, for as long as agents run.
2. **A join counted as successful whenever the internet answered within 20 s,
   whether or not the Mac had moved.** `hotspot.rs:213-220`. *Scenario:*
   `networksetup` does nothing, the old Wi-Fi's internet comes back within the
   window, and the code records "joined": the pill says Hotspot, background
   refreshes stop, and when agents finish the Wi-Fi is cycled for nothing. The
   opposite case: the Mac never left a dead network and the user is told
   "Joined the hotspot, but it has no internet."
3. **Error output was thrown away.** `run()` at `hotspot.rs:176-186` sends
   stderr to `/dev/null`, and any non-zero exit becomes "Couldn't run
   networksetup." *Scenario:* a failure printed on stderr reads as success,
   then shows up 20 s later as the wrong "no internet" message.
4. **No timeouts, so one hung command stopped the feature until restart.**
   `run()` and `online()`'s DNS lookup (`hotspot.rs:87-92`) can block with no
   time limit. While they run, `networking` stays `true`
   (`monocode-gpui/src/keep_awake.rs:465,507`). *Scenario:* `networksetup`
   hangs (stuck Wi-Fi driver, Keychain prompt) or `getaddrinfo` stalls. After
   that there are no more probes, joins or leaves until MonoCode restarts.
5. **The "joined" flag lived only in memory.** `hotspot.rs:31`. *Scenario:*
   MonoCode quits, crashes or updates while on the hotspot. The next launch
   doesn't know, so the Mac stays on the phone's data indefinitely, and
   background refreshes resume on cellular data.
6. **Leaving always cycled Wi-Fi power, even when that wasn't needed.**
   `hotspot.rs:224-230` and the settings test at `pages/hotspot.rs:199-201`.
   *Scenario:* the user already moved the Mac back, or turned Wi-Fi off, and
   agents finish: their working connection drops, or Wi-Fi turns back on
   against their wishes. "Test now" with the hotspot out of range still cycles
   Wi-Fi and drops a working connection. A test can also run at the same time
   as an automatic join, because nothing serialized them.
7. **Retries ran forever at a fixed pace and couldn't tell permanent failures
   from passing ones.** `hotspot.rs:49-59`. Each attempt runs
   `-setairportnetwork`, which drops the current association. *Scenario:* a
   wrong password or a "require admin to change networks" policy is retried
   every minute forever. The 60 s delay is measured from when the attempt
   started, so a 50 s attempt is retried 10 s after it fails, and restarting
   agents resets the spacing (`hotspot.rs:40-43`). The red tests in
   "How each fix is tested" fail on the old code.
8. **A spurious join after waking from sleep.** There was no wake grace. After
   a wake, Wi-Fi takes several seconds to reassociate, and longer on
   enterprise or weak networks. **[general]** `Instant` does not advance
   during sleep, but the first failed probes after a wake start the 15 s
   offline clock. *Scenario:* the user opens the lid, home Wi-Fi takes 20 s to
   return, and MonoCode has already moved the Mac to the phone.
9. **The probe was slow and narrow.** `hotspot.rs:80-93`. It made sequential
   3 s connects to two IPv4 hosts, then an untimed DNS lookup. It had no IPv6
   path. *Scenario:* on a network that blocks 1.1.1.1 and 8.8.8.8 and has a
   slow resolver, the probe answers "offline" (or hangs, see 4) and the Mac
   leaves a working network. On an IPv6-only network, the probe depends on
   DNS64 for the one named host.
10. **Silent failures and no logs.** Errors cleared on the first good probe
    (`keep_awake.rs:494-496`). The popover hard-coded "Trying again every
    minute". Failure kinds collapsed into three strings. The settings page
    had no live status, and nothing was written to disk. *Scenario:* a user
    reports "it didn't join" and there is nothing to look at.
11. **Wi-Fi switched off showed a misleading error.** `join()` didn't check
    power. *Scenario:* Wi-Fi is off. The user sees "isn't in range" or a
    generic failure instead of "Wi-Fi is off".
12. **The hotspot password is passed as a command-line argument** to
    `networksetup` (`hotspot.rs:205-208`), so other local processes can read
    it with `ps` for the moment the join runs. **[code]** Not fixed: only
    CoreWLAN avoids it, and CoreWLAN needs Location access (see "Join
    mechanism").
13. **Opening the lid leaves even while agents still work**
    (`keep_awake.rs:370`). *Scenario:* home Wi-Fi is still down, so the Mac
    goes offline, and joins the hotspot again 15 s or more later. This is
    documented behavior ("or you open the lid"), so it is kept. Each fix
    above makes the rejoin quick and logged. Listed as a follow-up.
14. **`network_drop.rs` waits with no time limit.** It probes every 5 s per
    cut-off conversation until the internet returns. This is cheap, and it now
    shares the faster, bounded probe. Left as is.

## Join mechanism

`networksetup -setairportnetwork <dev> <ssid> [password]` still exists on
macOS 27 (`networksetup -printcommands`). **[mac]** Notes **[general]**:

- It needs the network to answer a scan, so idle iPhone hotspots fail
  (defect 1).
- It can exit 0 and still fail, and prints the error text. Common codes:
  -3900 (often a wrong password), -3905 (timed out or out of range), -3924
  and -3925 (bad key or supplicant timeout), -3930 (not permitted, for
  example "Require administrator authorization to change networks").
- It works with open, WPA2 and WPA3 networks. "Maximize Compatibility" on the
  iPhone switches the hotspot to 2.4 GHz WPA2, which older Macs see more
  reliably.
- It does nothing useful with Wi-Fi powered off.

Alternatives:

- **CoreWLAN** (`CWInterface.associate(to:password:)`) needs a `CWNetwork`
  from a scan. Since macOS 14.4, scan results and `ssid()` are `nil` without
  Location Services access. That means an `NSLocationUsageDescription` in the
  app's Info.plist (outside this feature's files), a system prompt, and the
  user keeping Location on for MonoCode. It also can't wake an idle iPhone
  hotspot. It would keep the password out of `ps` and give typed errors.
  **Not worth it for this change:** it doesn't fix defect 1, and it adds an
  app-wide permission. The new `Wifi` trait is the seam it would plug into.
- `wdutil` needs `sudo`. `scutil` and `networksetup -setnetworkserviceenabled`
  can't pick an SSID. Raising a saved network's priority changes the user's
  saved networks, which is off limits.

## What changed

- `keep_awake/hotspot/join.rs` holds the join and leave sequences behind a
  `Wifi` trait, plus a `Clock` trait. `JoinError` has a kind for each failure.
  A join:
  1. checks that a Wi-Fi device exists and is powered on;
  2. records the current link;
  3. runs the join and classifies its stdout and stderr;
  4. waits up to 15 s until the link has moved to the hotspot (SSID when
     macOS shows it, otherwise the iPhone subnet or a changed lease and
     router);
  5. waits up to 20 s for the internet;
  6. checks that the link is still the hotspot.

  A leave doesn't touch Wi-Fi when it is off or the Mac has already moved, and
  reports when macOS went straight back to the hotspot.
- `keep_awake/hotspot/cmd.rs` runs commands with a timeout and keeps both
  output streams. The password is removed from any output before it is
  classified or logged.
- `keep_awake/hotspot/probe.rs` is the internet check. It connects in
  parallel to six addresses across four providers, over IPv4 and IPv6, plus
  `api.anthropic.com` through DNS. One answer is enough, and the whole check
  is bounded even if DNS hangs.
- `keep_awake/hotspot/record.rs` holds two files:
  - `keep-awake-hotspot.json` records what this app joined, so a later launch
    can still leave. It is written atomically and deleted after a leave.
  - `keep-awake-hotspot.log` is a JSON-lines log of every decision, rotated
    at 256 KB.
- The state machine `Hotspot`:
  - backs off between attempts: 20 s, 40 s, 80 s, 160 s, then every 5 min,
    with at most 10 attempts per outage;
  - gives up at once on a wrong password, an admin-only policy, or no Wi-Fi
    hardware;
  - measures retries from the end of an attempt, and keeps the spacing when
    agents restart;
  - holds off 30 s after a wake;
  - after a restart, waits 10 s before leaving a hotspot an earlier run
    joined;
  - emits a structured `Event` for every decision;
  - gives `summary()` the one-line status shown in the UI.
- GUI driver: persists and restores the join record, detects sleep (wall
  clock against monotonic clock), logs every event, shows the live status in
  the popover, and publishes it to Settings. Changing the hotspot settings
  resets the backoff.
- Settings: a live "Status" row. "Test now" refuses while MonoCode has the Mac
  on the hotspot, and leaves only when the join may actually have moved the
  Mac. Automatic joins and tests run one at a time behind a lock. The copy
  explains the idle-hotspot catch.

## How each fix is tested

`cargo test --manifest-path crates/monocode-core/Cargo.toml --lib keep_awake`.
The fakes are a scripted `Wifi` and a manual `Clock`. Nothing touches the real
interface.

| Defect | Test |
|---|---|
| 1, 7 | `backs_off_and_caps_attempts`, `retry_waits_from_the_failure_not_the_attempt`, `first_retry_is_quicker_than_a_minute`, `restarting_agents_keeps_the_retry_spacing`, `gives_up_at_once_on_a_wrong_password`, `settings_change_resets_the_backoff` |
| 2 | `join_fails_when_the_mac_never_moved_even_if_internet_answers`, `join_succeeds_on_an_iphone_subnet`, `join_trusts_a_visible_ssid`, `join_reports_macos_switching_back` |
| 3, 11 | `classifies_networksetup_output`, `join_stops_when_wifi_is_off`, `join_reports_no_lease` |
| 4 | `a_hung_command_times_out`, `a_hung_probe_does_not_stall_the_answer` |
| 5 | `the_join_record_survives_a_restart`, `a_restored_join_leaves_after_the_grace` |
| 6 | `leave_skips_wifi_when_already_moved_or_off`, `leave_reports_macos_rejoining_the_hotspot` |
| 8 | `no_join_right_after_a_wake`, `detects_sleep_from_the_clocks` |
| 9 | `a_hung_probe_does_not_stall_the_answer`, `one_answer_is_enough` |
| 10 | `every_decision_is_an_event`, `summary_says_what_it_is_doing`, `log_rotates` |

The red tests for 7 (`retry_waits_from_the_failure…`, `first_retry_is_quicker…`,
`restarting_agents_keeps…`) were run against the old `Hotspot` through its old
API, and all three failed there. The other tests target seams the old code
didn't have, so they can't compile against it. They encode the scenarios
above.

## Not verified (needs a real phone)

- Whether `-setairportnetwork` finds an iPhone hotspot whose Hotspot screen is
  closed, on macOS 27.
- The exact text and stream `networksetup` uses for each failure on macOS 27.
  The classifier matches the codes and phrases listed above, and anything else
  becomes "Couldn't join: <first line>".
- How fast DHCP gives a lease on a hotspot (the 15 s association window).
- Which network macOS picks after `leave` cycles Wi-Fi.
- The wake-detection threshold against real sleep and wake.

## Manual checklist (with your phone)

Before you start, set the hotspot in Settings › General › If Wi-Fi drops and
start an agent that runs for several minutes. Follow along with
`tail -f ~/Library/Application\ Support/<MonoCode data dir>/keep-awake-hotspot.log`.

1. **Happy path.** Open the iPhone's Personal Hotspot screen. Turn the Mac's
   home router off, or unplug its WAN. Within about 20 s the log shows
   `offline`, then `join`, then `joined` with a `172.20.10.x` router, and the
   pill reads "· Hotspot". Turn the router back on and let the agent finish:
   the log shows `leave` (`agents_done`), then `left` (`cycled`), and the Mac
   is back on home Wi-Fi.
2. **Idle hotspot.** Lock the phone and wait 2 minutes, then repeat step 1.
   Note whether the first join finds it. Expect `failed` with `not_found`, and
   retries at about 20 s, 40 s and 80 s. Unlock the phone and open the Hotspot
   screen: the next retry should join.
3. **Maximize Compatibility.** Repeat step 2 with it on (Personal Hotspot ›
   Maximize Compatibility).
4. **Wrong password.** Save a wrong password in Settings and run step 1:
   expect a single attempt, then `gave_up` with `wrong_password`, and the
   status says to check the password. Fix the password: the status clears and
   the next drop joins.
5. **Wi-Fi off.** Turn Wi-Fi off from the menu bar while an agent runs: the
   status says Wi-Fi is off, and MonoCode doesn't turn it on.
6. **Restart while joined.** After step 1 joins, quit MonoCode from the menu
   and relaunch it with no agents running. About 10 s later the log shows
   `restored`, then `leave` (`agents_done`), then `left`.
7. **You moved it yourself.** After a join, pick home Wi-Fi from the menu bar,
   then let the agent finish: expect `left` with `already_moved`, and no Wi-Fi
   blip.
8. **Sleep.** With the lid hold off, close the lid for a minute and open it:
   the log shows `woke`, and there is no join in the first 30 s while home
   Wi-Fi returns.
9. **Test now, out of range.** Turn the phone's hotspot off and press "Test
   now": you get "isn't in range…" and the Mac's current Wi-Fi doesn't drop.
10. **Phone without data.** Turn cellular data off on the phone, with the
    hotspot still on: expect `failed` with `no_internet` after about 35 s,
    and backoff after that.
