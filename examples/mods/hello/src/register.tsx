import { Badge, Box, Button, Text } from "monocode";
import type { Register } from "../../../../src/features/mods/api/types";

export const register: Register = (on, $) => {
  const turns = $.state.atom("turns", 0);
  const hidden = $.state.atom<string[]>("hidden", []);

  $.ui.open({ id: "hello", title: "Hello", site: "SidebarTab", icon: "puzzle", focus: false });
  $.ui.status("turns", "hello · 0 turns", { tone: "accent" });

  on("turn.complete", null, ($, e) => {
    $.state.update(turns, (n) => n + 1);
    const n = $.state.read(turns);
    $.ui.status("turns", `hello · ${n} turn${n === 1 ? "" : "s"}`, { tone: "accent" });
    $.ui.toast(`Turn done in ${(e.durationMs / 1000).toFixed(1)}s`, { tone: "success" });
  });

  on("ui.render", { component: "SidebarTab" }, ($) => (
    <Box flexDirection="column" gap={2} padding={2}>
      <Text bold>Hello from a mod</Text>
      <Box flexDirection="row" gap={2} alignItems="center">
        <Text color="muted">Turns this load</Text>
        <Badge label={String($.state.read(turns))} tone="accent" />
      </Box>
      <Button label="Say hi" onPress={() => $.ui.toast("Hi!", { tone: "info" })} />
    </Box>
  ));

  on("ui.render", { component: "AbovePrompt" }, ($, e) => {
    const sessionId = e.sessionId ?? "";
    if ($.state.read(hidden).includes(sessionId)) return null;
    return (
      <Box flexDirection="row" alignItems="center" justifyContent="between" gap={2}>
        <Text color="muted">hello is loaded for this session</Text>
        <Button
          plain
          icon="x"
          title="Hide"
          onPress={() => $.state.update(hidden, (ids) => [...ids, sessionId])}
        />
      </Box>
    );
  });
};
