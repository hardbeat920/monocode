import { expect, it } from "vitest";
import {
  REMOTE_PROVIDERS,
  isRemoteProvider,
  requireHostDescriptor,
} from "../src/features/connections/model/protocol";
import { hostProviders } from "./providers";
import { HARNESSES } from "../src/features/sessions/model/session";

it("exposes every local harness through the remote host contract", () => {
  expect(Object.keys(hostProviders).sort()).toEqual(
    [...REMOTE_PROVIDERS].sort(),
  );
  expect([...REMOTE_PROVIDERS].sort()).toEqual([...HARNESSES].sort());
  for (const provider of REMOTE_PROVIDERS) {
    expect(isRemoteProvider(provider)).toBe(true);
    expect(hostProviders[provider].send).toBeTypeOf("function");
    expect(hostProviders[provider].cancel).toBeTypeOf("function");
    expect(hostProviders[provider].bind).toBeTypeOf("function");
    expect(hostProviders[provider].approve).toBeTypeOf("function");
  }
  expect(
    requireHostDescriptor({
      protocolVersion: 1,
      environmentId: "host",
      name: "fixture",
      providers: [...REMOTE_PROVIDERS],
      capabilities: [],
    }).providers,
  ).toHaveLength(REMOTE_PROVIDERS.length);
});
