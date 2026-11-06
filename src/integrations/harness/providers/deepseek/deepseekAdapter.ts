import {
  bindDeepseekSession,
  cancelDeepseekTurn,
  forgetDeepseekSession,
  respondDeepseekApproval,
  sendDeepseekTurn,
  steerDeepseekTurn,
  stopDeepseekSession,
} from "./deepseek";
import { refreshDeepseekCatalog } from "./deepseekCatalog";
import { registerHarness, type HarnessAdapter } from "../../core/registry";

export const deepseekAdapter: HarnessAdapter = {
  id: "deepseek",
  live: true,
  canSteer: false,
  sendTurn: sendDeepseekTurn,
  steerTurn: steerDeepseekTurn,
  cancelTurn: cancelDeepseekTurn,
  respondApproval: respondDeepseekApproval,
  stopSession: stopDeepseekSession,
  forgetSession: forgetDeepseekSession,
  bindSession: bindDeepseekSession,
  refreshCatalog: refreshDeepseekCatalog,
};

let registered = false;

export function ensureDeepseekRegistered(): void {
  if (registered) return;
  registerHarness(deepseekAdapter);
  registered = true;
}
