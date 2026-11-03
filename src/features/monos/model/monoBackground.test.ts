// @vitest-environment happy-dom
import { afterEach, expect, it } from "vitest";
import { CHAT_BACKGROUND_OPACITY_DEFAULT } from "../../settings/model/appearance";
import { saveProjectChatBackgroundSettings } from "../../projects/model/projectChatBackground";
import { monoBackgroundKey, monoChatBackground } from "./monoBackground";

afterEach(() => localStorage.clear());

it("always shows a Mono's background as a dimmed Haze", () => {
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
    emptyOpacity: CHAT_BACKGROUND_OPACITY_DEFAULT,
    sessionOpacity: CHAT_BACKGROUND_OPACITY_DEFAULT,
    scope: "all",
    effect: "gradient-blur",
  });
});
