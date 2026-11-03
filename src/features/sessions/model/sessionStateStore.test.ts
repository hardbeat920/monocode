// @vitest-environment happy-dom
import { act, createElement, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { newSession } from "./session";
import {
  SessionStateContext,
  SessionStateStore,
  useSessionSnapshot,
  useWorkspaceSessionState,
} from "./sessionStateStore";
import { applyHarnessEvents } from "../../../integrations/harness/core/apply";
import { updateSessionState } from "../../../app/model/sessionState";

afterEach(() => vi.unstubAllGlobals());

it("publishes tokens to the visible session without rendering the shell or hidden panes", () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const initial = [newSession("claude", "/one"), newSession("codex", "/two")];
  let fallbacks = initial;
  const renders = [0, 0];
  let shellRenders = 0;
  let store!: SessionStateStore;
  let select!: (index: number) => void;
  const host = document.createElement("div");
  const root = createRoot(host);
  function Pane({ index, visible }: { index: number; visible: boolean }) {
    const session = useSessionSnapshot(fallbacks[index], visible);
    renders[index]++;
    return createElement(
      "p",
      { "data-busy": String(session.busy) },
      session.blocks.at(-1)?.text,
    );
  }
  function Workspace() {
    const state = useWorkspaceSessionState(() => initial);
    store = state.store;
    const [selected, setSelected] = useState(0);
    select = setSelected;
    shellRenders++;
    return createElement(
      SessionStateContext.Provider,
      { value: store },
      ...initial.map((_, index) =>
        createElement(Pane, { key: index, index, visible: index === selected }),
      ),
    );
  }
  act(() => root.render(createElement(Workspace)));
  const baseline = { shell: shellRenders, panes: [...renders] };
  for (let n = 0; n < 10; n++) {
    act(() =>
      store.update(
        (sessions) =>
          sessions.map((session) =>
            applyHarnessEvents(session, [
              { type: "message.delta", text: `chunk${n} ` },
            ]),
          ),
        true,
      ),
    );
  }
  expect(shellRenders).toBe(baseline.shell);
  expect(renders[0]).toBe(baseline.panes[0] + 10);
  expect(renders[1]).toBe(baseline.panes[1]);
  expect(store.ref.current[1].blocks[0].text).toContain("chunk9");
  act(() => select(1));
  expect(shellRenders).toBe(baseline.shell + 1);
  // Token-only updates do not refresh the parent's fallback. Hiding its
  // pane must keep the output already shown instead of restoring that draft.
  expect(host.querySelectorAll("p")[0].textContent).toContain("chunk9");
  expect(host.querySelectorAll("p")[1].textContent).toContain("chunk9");
  const afterSwitch = [...renders];
  act(() =>
    store.update(
      (sessions) =>
        sessions.map((session) =>
          applyHarnessEvents(session, [
            { type: "message.delta", text: "latest" },
          ]),
        ),
      true,
    ),
  );
  expect(renders[0]).toBe(afterSwitch[0]);
  expect(renders[1]).toBe(afterSwitch[1] + 1);
  expect(host.querySelectorAll("p")[0].textContent).not.toContain("latest");
  expect(host.querySelectorAll("p")[1].textContent).toContain("latest");
  act(() =>
    store.setState((sessions) => {
      fallbacks = sessions.map((session) => ({ ...session, busy: false }));
      return fallbacks;
    }),
  );
  expect(shellRenders).toBe(baseline.shell + 2);
  expect(host.querySelectorAll("p")[0].dataset.busy).toBe("false");
  expect(host.querySelectorAll("p")[0].textContent).toContain("latest");
  act(() => root.unmount());
});

it("keeps edits and synchronous runtime updates ordered around token batches", () => {
  const store = new SessionStateStore([newSession("claude", "/repo")]);
  const observed: string[] = [];
  const stop = store.observe(() =>
    observed.push(store.snapshot()[0].blocks.at(-1)?.text ?? ""),
  );
  store.setState((sessions) =>
    sessions.map((session) => ({ ...session, title: "Edited" })),
  );
  updateSessionState(store.ref, store.setState, (sessions) =>
    sessions.map((session) => ({ ...session, providerAccountId: "work" })),
  );
  store.update(
    (sessions) =>
      sessions.map((session) =>
        applyHarnessEvents(session, [{ type: "message.delta", text: "once" }]),
      ),
    true,
  );
  expect(store.snapshot()[0]).toMatchObject({
    title: "Edited",
    providerAccountId: "work",
    blocks: [{ text: "once" }],
  });
  expect(store.ref.current).toBe(store.snapshot());
  expect(observed).toEqual(["", "", "once"]);
  stop();
});
