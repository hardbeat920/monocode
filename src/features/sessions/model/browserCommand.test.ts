import { describe, expect, it } from "vitest";
import {
  BROWSER_ASK_NOTE,
  browserGrantChange,
  browserTurnAccess,
  browserTurnEligible,
  consumeBrowserCommand,
  inheritsMonoBrowserGrant,
  withBrowserAskNote,
} from "./browserCommand";
import { consumeOperatorCommand } from "./operatorCommand";

describe("/browser", () => {
  it("takes a leading command and leaves the prompt", () => {
    expect(consumeBrowserCommand("/browser open example.com")).toEqual({
      text: "open example.com",
      matched: true,
    });
    expect(consumeBrowserCommand("  /BROWSER\nlook it up")).toEqual({
      text: "look it up",
      matched: true,
    });
    expect(consumeBrowserCommand("/browser")).toEqual({
      text: "",
      matched: true,
    });
  });

  it("ignores the word elsewhere and longer commands", () => {
    for (const text of ["use /browser", "/browsers", "/browser-extra x"])
      expect(consumeBrowserCommand(text)).toEqual({ text, matched: false });
  });

  it("keeps the Operator prefix in front of it for the operator parser", () => {
    const browser = consumeBrowserCommand("/operator /browser list tabs");
    expect(browser).toEqual({ text: "/operator list tabs", matched: true });
    expect(consumeOperatorCommand(browser.text)).toEqual({
      text: "list tabs",
      matched: true,
    });
  });
});

describe("/browser grant", () => {
  it("switches when alone", () => {
    expect(browserGrantChange(false, true)).toBe(true);
    expect(browserGrantChange(true, true)).toBe(false);
  });

  it("with a prompt grants once and leaves an existing grant alone", () => {
    expect(browserGrantChange(false, false)).toBe(true);
    expect(browserGrantChange(true, false)).toBeNull();
  });
});

describe("browser access for a turn without /browser", () => {
  const base = {
    toggleOn: true,
    access: "ask" as const,
    granted: false,
    eligible: true,
    turnedOff: false,
  };

  it("carries nothing while the main switch is off", () => {
    expect(browserTurnAccess({ ...base, toggleOn: false })).toBeNull();
    expect(
      browserTurnAccess({ ...base, toggleOn: false, granted: true }),
    ).toBeNull();
  });

  it("keeps a grant the thread already has, eligible or not", () => {
    expect(browserTurnAccess({ ...base, granted: true })).toBe("granted");
    expect(browserTurnAccess({ ...base, granted: true, eligible: false })).toBe(
      "granted",
    );
    expect(
      browserTurnAccess({ ...base, access: "always", granted: true }),
    ).toBe("granted");
  });

  it("asks in Ask mode and grants first in Always on", () => {
    expect(browserTurnAccess(base)).toBe("ask");
    expect(browserTurnAccess({ ...base, access: "always" })).toBe("grant");
  });

  it("grants a Full access thread first, even in Ask mode", () => {
    expect(browserTurnAccess({ ...base, fullAccess: true })).toBe("grant");
    expect(browserTurnAccess({ ...base, fullAccess: false })).toBe("ask");
    expect(
      browserTurnAccess({ ...base, fullAccess: true, eligible: false }),
    ).toBeNull();
    expect(
      browserTurnAccess({ ...base, fullAccess: true, toggleOn: false }),
    ).toBeNull();
    expect(
      browserTurnAccess({ ...base, fullAccess: true, turnedOff: true }),
    ).toBe("ask");
  });

  it("gives orchestration, managed and app-request turns nothing", () => {
    expect(browserTurnAccess({ ...base, eligible: false })).toBeNull();
    expect(
      browserTurnAccess({ ...base, access: "always", eligible: false }),
    ).toBeNull();
  });

  it("asks instead of granting again a thread the user turned off", () => {
    expect(
      browserTurnAccess({ ...base, access: "always", turnedOff: true }),
    ).toBe("ask");
    expect(browserTurnAccess({ ...base, turnedOff: true })).toBe("ask");
  });
});

describe("browserTurnEligible", () => {
  const regular = { orchestration: false };

  it("lets a turn the user sends from a regular thread use the browser", () => {
    expect(browserTurnEligible(regular)).toBe(true);
  });

  it("keeps agent-sent, managed and orchestration turns out", () => {
    // A session an agent starts through the app, monitored or not.
    expect(browserTurnEligible({ ...regular, browserGrantOnly: true })).toBe(
      false,
    );
    // A follow-up an agent hands a session (`sessions.send`).
    expect(browserTurnEligible({ ...regular, appRequestId: "r1" })).toBe(false);
    expect(browserTurnEligible({ ...regular, managed: true })).toBe(false);
    expect(browserTurnEligible({ orchestration: true })).toBe(false);
  });

  it("never grants an agent-launched Full access or Always on session", () => {
    const eligible = browserTurnEligible({
      ...regular,
      browserGrantOnly: true,
    });
    const turn = {
      toggleOn: true,
      granted: false,
      eligible,
      turnedOff: false,
    };
    expect(
      browserTurnAccess({ ...turn, access: "ask", fullAccess: true }),
    ).toBeNull();
    expect(browserTurnAccess({ ...turn, access: "always" })).toBeNull();
    // A grant it inherited from the Mono that started it still counts.
    expect(
      browserTurnAccess({ ...turn, access: "always", granted: true }),
    ).toBe("granted");
  });
});

describe("ask note", () => {
  it("goes inside the browser block", () => {
    expect(
      withBrowserAskNote(
        "<monocode_browser>\nUse the tools.\n</monocode_browser>",
      ),
    ).toBe(
      `<monocode_browser>\nUse the tools.\n${BROWSER_ASK_NOTE}\n</monocode_browser>`,
    );
  });

  it("follows a block without the closing tag", () => {
    expect(withBrowserAskNote("Use the tools.")).toBe(
      `Use the tools.\n${BROWSER_ASK_NOTE}`,
    );
  });
});

describe("inheritsMonoBrowserGrant", () => {
  const base = { monoGranted: true, granted: false, turnedOff: false };

  it("passes the Mono's grant to a session it started", () => {
    expect(inheritsMonoBrowserGrant(base)).toBe(true);
  });

  it("passes nothing while the Mono has no grant", () => {
    expect(inheritsMonoBrowserGrant({ ...base, monoGranted: false })).toBe(
      false,
    );
  });

  it("leaves a session the user turned off alone", () => {
    expect(inheritsMonoBrowserGrant({ ...base, turnedOff: true })).toBe(false);
  });

  it("does not grant a session that already has access", () => {
    expect(inheritsMonoBrowserGrant({ ...base, granted: true })).toBe(false);
  });
});
