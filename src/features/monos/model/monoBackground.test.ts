// @vitest-environment happy-dom
import { afterEach, expect, it } from "vitest";
import { saveProjectChatBackgroundSettings } from "../../projects/model/projectChatBackground";
import { monoBackgroundKey, monoChatBackground } from "./monoBackground";

afterEach(() => localStorage.clear());

it("uses a Mono's stored effect and visibility on its entire conversation", () => {
  expect(monoChatBackground("a")).toBeNull();
  saveProjectChatBackgroundSettings(monoBackgroundKey("a"), {
    path: "/bg/mono.png",
    emptyOpacity: 0.6,
    sessionOpacity: 0.5,
    scope: "empty",
    effect: "dither",
  });
  expect(monoChatBackground("a")).toEqual({
    path: "/bg/mono.png",
    emptyOpacity: 0.6,
    sessionOpacity: 0.5,
    scope: "all",
    effect: "dither",
  });
});
