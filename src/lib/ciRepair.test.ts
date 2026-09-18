import { expect, it } from "vitest";
import { buildCiRepairRequest } from "./ciRepair";

it("keeps failure evidence for the agent without passing successful steps and timestamps", () => {
  const request = buildCiRepairRequest({
    repo: "acme/web",
    number: 42,
    headOid: "abc123",
    evidence: [
      {
        name: "Windows",
        workflow: "CI",
        state: "fail",
        url: "https://github.com/acme/web/actions/runs/1/job/2",
        startedAt: "2030-01-01T00:00:00Z",
        completedAt: "2030-01-01T00:01:00Z",
        details: {
          steps: [
            {
              name: "Install dependencies",
              state: "pass",
              startedAt: null,
              completedAt: null,
            },
            {
              name: "Run tests",
              state: "fail",
              startedAt: null,
              completedAt: null,
            },
          ],
          annotations: [
            {
              path: "src/app.test.ts",
              line: 42,
              message: "Expected 2, received 1",
              level: "failure",
            },
          ],
          notice: "Full logs are available on GitHub.",
        },
      },
    ],
  });
  expect(request.text).toBe("Fix 1 failed CI check for acme/web PR #42.");
  expect(request.prompt).toContain("abc123");
  expect(request.prompt).toContain("Run tests");
  expect(request.prompt).toContain("src/app.test.ts");
  expect(request.prompt).toContain("Expected 2, received 1");
  expect(request.prompt).toContain(
    "https://github.com/acme/web/actions/runs/1/job/2",
  );
  expect(request.prompt).toContain("Full logs are available on GitHub.");
  expect(request.prompt).not.toContain("Install dependencies");
  expect(request.prompt).not.toContain("startedAt");
  expect(request.prompt).not.toContain("2030-01-01");
});
