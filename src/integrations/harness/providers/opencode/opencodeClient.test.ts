import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  harnessHttp: vi.fn(),
  openHarnessSse: vi.fn(),
  watchSse: vi.fn(),
}));

vi.mock("../../core/child", () => ({
  closeHarnessSse: vi.fn(),
  harnessHttp: mocks.harnessHttp,
  openHarnessSse: mocks.openHarnessSse,
  watchSse: mocks.watchSse,
}));

import { OpenCodeClient } from "./opencodeClient";

describe("OpenCodeClient.summarizeSession", () => {
  beforeEach(() => {
    mocks.harnessHttp.mockReset();
    mocks.harnessHttp.mockResolvedValue({ status: 200, body: "true" });
    mocks.openHarnessSse.mockReset();
    mocks.openHarnessSse.mockResolvedValue(undefined);
    mocks.watchSse.mockReset();
  });

  it("calls the native session summarize endpoint with the selected model", async () => {
    const client = new OpenCodeClient("http://127.0.0.1:4096", "/repo");

    await client.summarizeSession("session/a", {
      providerID: "openai",
      modelID: "gpt-5.4",
    });

    expect(mocks.harnessHttp).toHaveBeenCalledWith({
      url: "http://127.0.0.1:4096/session/session%2Fa/summarize?directory=%2Frepo",
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "x-opencode-directory": "%2Frepo",
      },
      body: JSON.stringify({ providerID: "openai", modelID: "gpt-5.4" }),
      timeoutMs: 30 * 60_000,
    });
  });
});

describe("OpenCodeClient v2", () => {
  beforeEach(() => {
    mocks.harnessHttp.mockReset();
    mocks.harnessHttp.mockResolvedValue({ status: 204, body: "" });
    mocks.openHarnessSse.mockReset();
    mocks.openHarnessSse.mockResolvedValue(undefined);
    mocks.watchSse.mockReset();
  });

  it("creates sessions with a location and v2 permission rules", async () => {
    mocks.harnessHttp.mockResolvedValueOnce({
      status: 200,
      body: JSON.stringify({ data: { id: "ses_1" } }),
    });
    const client = new OpenCodeClient("http://127.0.0.1:4096", "/repo", "v2");

    await expect(
      client.createSession({
        permission: [{ permission: "*", pattern: "*", action: "ask" }],
      }),
    ).resolves.toEqual({ id: "ses_1" });
    expect(mocks.harnessHttp).toHaveBeenCalledWith(
      expect.objectContaining({
        method: "POST",
        url: "http://127.0.0.1:4096/api/session?directory=%2Frepo",
        body: JSON.stringify({
          location: { directory: "/repo" },
          permissions: [{ action: "*", resource: "*", effect: "ask" }],
        }),
      }),
    );
  });

  it("authenticates background service requests", async () => {
    mocks.harnessHttp.mockResolvedValueOnce({
      status: 200,
      body: JSON.stringify({ data: [] }),
    });
    const client = new OpenCodeClient(
      "http://127.0.0.1:4096",
      "/repo",
      "v2",
      "secret",
    );

    await client.listModels();

    expect(mocks.harnessHttp).toHaveBeenCalledWith(
      expect.objectContaining({
        headers: expect.objectContaining({
          Authorization: `Basic ${btoa("opencode:secret")}`,
        }),
      }),
    );
  });

  it("can request the global v2 catalog without a directory context", async () => {
    mocks.harnessHttp.mockResolvedValueOnce({
      status: 200,
      body: JSON.stringify({ data: [] }),
    });
    const client = new OpenCodeClient(
      "http://127.0.0.1:4096",
      "",
      "v2",
      "secret",
    );

    await client.listModels();

    expect(mocks.harnessHttp).toHaveBeenCalledWith(
      expect.objectContaining({
        url: "http://127.0.0.1:4096/api/model",
        headers: {
          Authorization: `Basic ${btoa("opencode:secret")}`,
        },
      }),
    );
  });

  it("sets model and agent before queueing a prompt", async () => {
    const client = new OpenCodeClient("http://127.0.0.1:4096", "/repo", "v2");

    await client.promptAsync({
      sessionID: "ses_1",
      model: { providerID: "openai", modelID: "gpt-5.4" },
      agent: "build",
      variant: "high",
      parts: [
        { type: "text", text: "Fix it" },
        {
          type: "file",
          mime: "image/png",
          filename: "bug.png",
          url: "data:image/png;base64,YWJj",
        },
      ],
    });

    expect(
      mocks.harnessHttp.mock.calls.map(([request]) => ({
        path: new URL(request.url).pathname,
        body: request.body ? JSON.parse(request.body) : undefined,
      })),
    ).toEqual([
      {
        path: "/api/session/ses_1/model",
        body: {
          model: {
            providerID: "openai",
            id: "gpt-5.4",
            variant: "high",
          },
        },
      },
      {
        path: "/api/session/ses_1/agent",
        body: { agent: "build" },
      },
      {
        path: "/api/session/ses_1/prompt",
        body: {
          text: "Fix it",
          files: [{ uri: "data:image/png;base64,YWJj", name: "bug.png" }],
          delivery: "queue",
        },
      },
    ]);

    await client.promptAsync({
      sessionID: "ses_1",
      model: { providerID: "openai", modelID: "gpt-5.4" },
      agent: "build",
      variant: "high",
      parts: [{ type: "text", text: "Continue" }],
    });
    expect(mocks.harnessHttp).toHaveBeenCalledTimes(4);
    expect(new URL(mocks.harnessHttp.mock.calls[3]![0].url).pathname).toBe(
      "/api/session/ses_1/prompt",
    );
  });

  it("normalizes v2 permission events and routes replies through the session", async () => {
    let receive: ((data: string) => void) | undefined;
    mocks.watchSse.mockImplementation(
      (_id: string, callback: (data: string) => void) => {
        receive = callback;
      },
    );
    const client = new OpenCodeClient("http://127.0.0.1:4096", "/repo", "v2");
    const events: Record<string, unknown>[] = [];
    await client.subscribeEvents("thread", (event) => events.push(event));

    receive?.(
      JSON.stringify({
        id: "evt_1",
        type: "permission.asked",
        data: {
          id: "per_1",
          sessionID: "ses_1",
          action: "bash",
          resources: ["npm test"],
        },
      }),
    );
    expect(events[0]).toMatchObject({
      type: "permission.asked",
      properties: {
        id: "per_1",
        sessionID: "ses_1",
        permission: "bash",
        patterns: ["npm test"],
      },
    });

    await client.replyPermission("per_1", "once");
    expect(mocks.harnessHttp).toHaveBeenCalledWith(
      expect.objectContaining({
        method: "POST",
        url: "http://127.0.0.1:4096/api/session/ses_1/permission/per_1/reply?directory=%2Frepo",
        body: JSON.stringify({ decision: "once" }),
      }),
    );
  });

  it("maps v2 forms onto questions and sends keyed answers", async () => {
    let receive: ((data: string) => void) | undefined;
    mocks.watchSse.mockImplementation(
      (_id: string, callback: (data: string) => void) => {
        receive = callback;
      },
    );
    const client = new OpenCodeClient("http://127.0.0.1:4096", "/repo", "v2");
    const events: Record<string, unknown>[] = [];
    await client.subscribeEvents("thread", (event) => events.push(event));
    receive?.(
      JSON.stringify({
        type: "form.created",
        data: {
          form: {
            id: "frm_1",
            sessionID: "ses_1",
            fields: [
              {
                key: "strategy",
                type: "multiselect",
                prompt: "Choose strategies",
                options: [{ label: "Fast" }, { label: "Safe" }],
              },
            ],
          },
        },
      }),
    );

    expect(events[0]).toMatchObject({
      type: "question.asked",
      properties: {
        id: "frm_1",
        sessionID: "ses_1",
        questions: [
          {
            id: "strategy",
            question: "Choose strategies",
            multiple: true,
          },
        ],
      },
    });
    await client.replyQuestion("frm_1", [["Fast", "Safe"]]);
    expect(mocks.harnessHttp).toHaveBeenCalledWith(
      expect.objectContaining({
        method: "POST",
        url: "http://127.0.0.1:4096/api/session/ses_1/form/frm_1/reply?directory=%2Frepo",
        body: JSON.stringify({
          answer: { strategy: ["Fast", "Safe"] },
        }),
      }),
    );
  });

  it("answers boolean form fields only on exact yes/true", async () => {
    let receive: ((data: string) => void) | undefined;
    mocks.watchSse.mockImplementation(
      (_id: string, callback: (data: string) => void) => {
        receive = callback;
      },
    );
    const client = new OpenCodeClient("http://127.0.0.1:4096", "/repo", "v2");
    await client.subscribeEvents("thread", () => undefined);
    const sendForm = (id: string) =>
      receive?.(
        JSON.stringify({
          type: "form.created",
          data: {
            form: {
              id,
              sessionID: "ses_1",
              fields: [{ key: "confirm", type: "boolean" }],
            },
          },
        }),
      );
    sendForm("frm_1");
    sendForm("frm_2");
    sendForm("frm_3");

    await client.replyQuestion("frm_1", [["yesterday"]]);
    await client.replyQuestion("frm_2", [["not true"]]);
    await client.replyQuestion("frm_3", [[" yes "]]);
    const bodies = mocks.harnessHttp.mock.calls.map(([input]) => input.body);
    expect(bodies.at(-3)).toBe(JSON.stringify({ answer: { confirm: false } }));
    expect(bodies.at(-2)).toBe(JSON.stringify({ answer: { confirm: false } }));
    expect(bodies.at(-1)).toBe(JSON.stringify({ answer: { confirm: true } }));
  });
});
