import { expect, it } from "vitest";
import { sonarLabel, sonarTransitionLabel } from "./sonarLabels";
it("uses readable Sonar labels while retaining the distinctions between severity scales", () => {
  expect(sonarLabel("CODE_SMELL")).toBe("Code smell");
  expect(sonarLabel("CRITICAL")).toBe("Critical");
  expect(sonarLabel("MAJOR")).toBe("Major");
  expect(sonarLabel("HIGH")).toBe("High");
  expect(sonarLabel("FALSE_POSITIVE")).toBe("False positive");
  expect(sonarLabel("OPEN")).toBe("Open");
  expect(sonarLabel("NEW_ENUM_VALUE")).toBe("New enum value");
  expect(sonarTransitionLabel("falsepositive")).toBe("False positive");
});
