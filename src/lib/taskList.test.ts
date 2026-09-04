import { describe, expect, it } from "vitest";
import {
  isTaskListToolName,
  legacyTaskListFromText,
  normalizeTaskListStatus,
  taskListFromToolInput,
  taskListProgressLabel,
  taskListText,
} from "./taskList";

describe("task lists", () => {
  it("normalizes provider status spellings", () => {
    expect(normalizeTaskListStatus("inProgress")).toBe("in_progress");
    expect(normalizeTaskListStatus("in_progress")).toBe("in_progress");
    expect(normalizeTaskListStatus("done")).toBe("completed");
    expect(normalizeTaskListStatus("skipped")).toBe("cancelled");
    expect(normalizeTaskListStatus("unknown")).toBe("pending");
  });

  it("normalizes todo-write tools from different harnesses", () => {
    expect(isTaskListToolName("TodoWrite")).toBe(true);
    expect(isTaskListToolName("update_todos")).toBe(true);
    expect(isTaskListToolName("edit")).toBe(false);
    expect(
      taskListFromToolInput("todowrite", {
        todos: [
          { id: "inspect", content: "Inspect", status: "completed" },
          { id: 2, activeForm: "Implementing", status: "inProgress" },
          { text: "Verify", status: "pending" },
        ],
      }),
    ).toEqual([
      { id: "inspect", text: "Inspect", status: "completed" },
      { id: "2", text: "Implementing", status: "in_progress" },
      { text: "Verify", status: "pending" },
    ]);
    expect(taskListFromToolInput("edit", { todos: [] })).toBeNull();
  });

  it("keeps a searchable text representation and reads legacy snapshots", () => {
    const items = [
      { text: "Inspect", status: "completed" as const },
      { text: "Implement", status: "in_progress" as const },
      { text: "Verify", status: "pending" as const },
    ];
    const text = taskListText(items);
    expect(text).toBe("[x] Inspect\n[~] Implement\n[ ] Verify");
    expect(legacyTaskListFromText(text)).toEqual(items);
    expect(legacyTaskListFromText("## Plan\n\n- Implement it")).toBeNull();
  });

  it("summarizes progress without counting cancelled tasks", () => {
    expect(
      taskListProgressLabel([
        { text: "One", status: "completed" },
        { text: "Two", status: "cancelled" },
      ]),
    ).toBe("Complete");
    expect(
      taskListProgressLabel([
        { text: "One", status: "completed" },
        { text: "Two", status: "pending" },
      ]),
    ).toBe("1 of 2");
  });
});
