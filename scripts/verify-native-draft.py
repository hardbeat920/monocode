#!/usr/bin/env python3
"""Drive the Draft panel through its self-test and check what the agent would see.

Needs a built debug binary on macOS (cargo build --profile native-dev in
crates/monocode-gpui). Opens a window for a few seconds.
"""
import argparse
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile

root = pathlib.Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument(
    "--binary",
    type=pathlib.Path,
    default=root / "crates/monocode-gpui/target/native-dev/monocode-gpui",
)
parser.add_argument("--keep", action="store_true", help="keep the output directory")
args = parser.parse_args()
if sys.platform != "darwin":
    sys.exit("The Draft panel is macOS only")
if not args.binary.exists():
    sys.exit(f"Build first: {args.binary} does not exist")
# A build without debug assertions has no self-test and would start the full
# app on the person's real MonoCode database instead.
if b"MONOCODE_DRAFT_SELFTEST" not in args.binary.read_bytes():
    sys.exit(f"{args.binary} has no self-test: build it with cargo build --profile native-dev, with debug assertions on")

out = pathlib.Path(tempfile.mkdtemp(prefix="monocode-draft-verify-"))
page = out / "page.html"
shutil.copy(root / "docs/native/examples/draft-spike.html", page)
actions = [
    {"draft": "$draft", "action": {"kind": "click", "reference": "@Add one"}},
    {"draft": "$draft", "action": {"kind": "click", "reference": "@Your name"}},
    {"draft": "$draft", "action": {"kind": "type", "text": "Ada", "replace": True}},
    {"debug": "takeover"},
    {"draft": "$draft", "action": {"kind": "click", "reference": "@Add one"}},
    {"draft": "$draft", "action": {"kind": "resume"}},
    {"draft": "$draft", "width": 375, "height": 700},
    {"debug": "note"},
    {"draft": "$draft", "action": {"kind": "wait", "seconds": 2}},
    {"debug": "send_later"},
    {"draft": "$draft", "action": {"kind": "wait", "seconds": 6}},
    {"draft": "$draft", "action": {"kind": "close"}},
]


def run(actions, source=page, start=None):
    # The draft is drawn inside a thread, as a card between two messages.
    env = dict(
        os.environ,
        MONOCODE_DRAFT_SELFTEST_START=start or "",
        MONOCODE_DRAFT_SELFTEST=str(source),
        MONOCODE_DRAFT_SELFTEST_THREAD="1",
        MONOCODE_DRAFT_SELFTEST_PANEL="900",
        MONOCODE_DRAFT_ACTIONS=json.dumps(actions),
    )
    app = subprocess.Popen([str(args.binary)], env=env,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    # A window behind another app stops drawing after its first frame, and
    # the checks need it to keep painting.
    subprocess.run(["osascript", "-e", "delay 0.5", "-e",
                    'tell application "System Events" to set frontmost of '
                    f"(first process whose unix id is {app.pid}) to true"],
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    if app.wait(timeout=420) != 0:
        sys.exit(f"The self-test exited with {app.returncode}")
    results = []
    n = 0
    while (path := out / f"{source.name}.{n}.json").exists():
        results.append(json.loads(path.read_text()))
        n += 1
    return results


steps = run(actions)
refs = {e["name"]: e["ref"] for e in steps[0]["elements"]}

failures = []


def check(condition, message):
    if not condition:
        failures.append(message)


opening = steps[0]
# A picture has the pixels of the display the window is on at that moment, and
# the window can change displays during a run.
def scale(step):
    return step["width"] / step["diagnostics"]["viewportWidth"]


check(opening.get("loaded") is True, "the page loads")
check(opening["diagnostics"]["ready"] is True, "diagnostics report ready")
check(opening["diagnostics"]["viewportWidth"] >= 500 and scale(opening) in (1, 2) and (out / "page.html.0.png").stat().st_size > 5000, "a real picture comes back")
check("ready" in [c["text"] for c in opening["console"]], "console output is captured")
check("Add one" in refs and "Your name" in refs, "controls are listed with refs")
placed = opening.get("placed") or [0, 0, 0, 0, 1]
check(placed[4] == 0 and placed[2] > 300, "the page is shown inside the thread")
check(abs(placed[3] - opening["contentHeight"]) <= 2, "the card is as tall as the page")
check("1" in steps[1]["text"].split("Add one")[1][:4], "a click reaches the page")
check("Hello, Ada" in steps[3]["text"], "typing replaces the input value")
check("trusted clicks: 0" in steps[3]["text"], "agent clicks are not trusted events")
check(steps[4].get("takeover") is True, "the test person takes over")
check("error" in steps[5] and "hand" in steps[5]["error"].lower(), "the agent is gated while the person drives")
check(steps[6].get("userControl") is False, "resume hands the page back")
resized = steps[7]
check(resized["diagnostics"]["viewportWidth"] == 375 and scale(resized) in (1, 2), "a resize relays the page out")
# A height only sets where the card starts; the card then follows the page.
check(resized["height"] == scale(resized) * resized["contentHeight"], "the card follows the page's height after a resize")
check(steps[8].get("pending") == 1, "a note pinned in the page is collected, not yet sent")
timed_out = steps[9]
check(timed_out.get("event") == "timeout" and timed_out["feedback"] == [], "a wait with nothing sent times out and carries no notes")
woken = steps[11]
check(woken.get("event") == "feedback", "sending wakes a waiting agent")
check([f["note"] for f in woken.get("feedback", [])] == ["Make this blue"], "the agent receives the pinned note")
check(timed_out.get("userActions") == ['clicked "Add one"'], "the agent learns what the person did by hand")
check(not any("Low contrast" in issue for issue in opening.get("issues", [])), "readable text is not flagged")
check(any("without a label" in issue for issue in opening.get("issues", [])), "the page is audited for missing labels")
check("closed" in steps[12], "close frees the draft")

# The optional controls kit: controls outside the content, bound to mc.state.
controls_page = out / "controls.html"
shutil.copy(root / "docs/native/examples/controls-kit-all.html", controls_page)
controls = run([
    {"draft": "$draft", "action": {"kind": "click", "reference": "@Uppercase"}},
    {"draft": "$draft", "action": {"kind": "click", "reference": "@Two"}},
    {"draft": "$draft", "action": {"kind": "click", "reference": "@center"}},
    {"draft": "$draft", "action": {"kind": "click", "reference": "@Increase Size"}},
    {"draft": "$draft", "action": {"kind": "click", "reference": "@Reset"}},
], controls_page)
check(controls[0]["console"] == [] and controls[0]["issues"] == [], "the controls kit renders with no console errors or audit issues")
check(controls[0].get("state", {}).get("size") == 14, "the agent sees the page's control values")
check(controls[1]["state"]["caps"] is True, "a checkbox changes the state")
check(controls[2]["state"]["mode"] == "b" and controls[3]["state"]["align"] == "center", "segmented and radio controls change the state")
check(controls[4]["state"]["size"] == 15, "a stepper steps")
check(controls[5]["state"] == controls[0]["state"], "a button resets the state")

# Anything the page runs can post the person's input itself. With no click or
# key press behind it, none of it takes control, pins a note or reaches the agent.
forged = run([{"debug": "forged"}])
check(forged[1].get("control") is False, "a page cannot take control for the person")
check(forged[1].get("pending") == 0, "a page cannot pin notes for the person")
check(forged[1].get("signal") is None, "a page cannot message the agent for the person")

# The web view draws over everything GPUI paints, so it has to step aside when
# a surface such as Settings is drawn in the thread's place, and show a still
# under a modal's scrim.
away = run([{"debug": "away"}, {"debug": "back"}, {"debug": "cover"}, {"debug": "back"}])
check((away[1].get("placed") or [0] * 5)[4] == 1, "the page hides when the thread is replaced")
check(away[2].get("placed") == away[0].get("placed"), "the page comes back where it was")
check((away[3].get("placed") or [0] * 5)[4] == 1, "a modal's scrim covers the page")
check(away[4].get("placed") == away[0].get("placed"), "the page comes back after the modal")

# Controls floating over the thread stay in front of the page, and take the
# pointer there.
floats = run([{"debug": "float"}, {"debug": "back"}])
holes = floats[1].get("holes") or []
check(len(holes) == 2, "the page has a hole for each control floating over it")
check(all(hole[5] == 0 for hole in holes), "a press on a floating control does not reach the page")
check(floats[2].get("holes") == [], "the page closes up once the controls go")
check((floats[1].get("placed") or [1] * 5)[4] == 0, "the page stays up around floating controls")

# A page whose process dies leaves an empty view for good: the card opens it again.
crashed = run([{"debug": "page"}, {"debug": "kill_page"}, {"debug": "hold:5"}, {"debug": "page"}])
check(crashed[1].get("painted") is True, "the page has drawn before its process goes")
check(crashed[4].get("pid") not in (None, crashed[2].get("pid")), "a page whose process died opens in a new one")
check(crashed[4].get("painted") is True and crashed[4].get("shown") is True, "the new page draws and shows")

# After minutes hidden WebKit empties the page's layers until its process
# commits new ones. A stalled process never does, as a swapped-out one may not
# for seconds: the card shows its placeholder, then opens the page again.
if os.environ.get("MONOCODE_VERIFY_LONG_HIDE"):
    stalled = run([
        {"debug": "page"}, {"debug": "away"}, {"debug": "hold:270"}, {"debug": "stop_page"},
        {"debug": "back"}, {"debug": "hold:1"}, {"debug": "page"}, {"debug": "hold:6"}, {"debug": "page"},
    ])
    check(stalled[7].get("painted") is False and stalled[7].get("blank") is True, "a page that comes back empty shows the placeholder")
    check(stalled[9].get("pid") != stalled[1].get("pid"), "a page that stays empty opens in a new process")
    check(stalled[9].get("painted") is True and stalled[9].get("blank") is False, "the new page draws")
    subprocess.run(["kill", "-CONT", str(stalled[4].get("pid"))], check=False)

# The agent keeps working on a draft whose conversation is in the background:
# the page runs out of sight for each request, then pauses again.
background = run([
    {"debug": "away"},
    {"draft": "$draft"},
    {"draft": "$draft", "action": {"kind": "click", "reference": "@Add one"}},
    {"draft": "$draft", "html": "<button>Fresh version</button>"},
    {"debug": "back"},
    {"draft": "$draft"},
])
check(background[2].get("loaded") is True and (out / "page.html.2.png").stat().st_size > 5000, "a snapshot works with the conversation in the background")
check("1" in background[3].get("text", "").split("Add one")[1][:4], "a click works with the conversation in the background")
check("Fresh version" in background[4].get("text", ""), "a new version loads with the conversation in the background")
check((background[4].get("placed") or [0] * 5)[4] == 1, "the page pauses again once the request is done")
check("Fresh version" in background[6].get("text", "") and (background[6].get("placed") or [1] * 5)[4] == 0, "the card shows the latest version when the person comes back")

# The agent opens a draft while the person is elsewhere: its card has never
# been on screen, so the page opens out of sight for the agent.
unseen = run([
    {"draft": "$draft", "action": {"kind": "click", "reference": "@Add one"}},
    {"debug": "back"},
    {"draft": "$draft"},
], start="away")
check(unseen[0].get("loaded") is True and "error" not in unseen[0], f"a draft opens with its card never on screen: {unseen[0].get('error', '')}")
check("1" in unseen[1].get("text", "").split("Add one")[-1][:4], "a click works before the card was ever on screen")
check((unseen[3].get("placed") or [1] * 5)[4] == 0, "the card shows the page when the person comes back")

if failures:
    print("FAILED:\n  " + "\n  ".join(failures))
    print(f"Output kept in {out}")
    sys.exit(1)
print("Draft panel verified")
if args.keep:
    print(f"Output in {out}")
else:
    shutil.rmtree(out)
