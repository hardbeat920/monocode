import {
  bindOpenCrabsSession,
  cancelOpenCrabsTurn,
  forgetOpenCrabsSession,
  respondOpenCrabsApproval,
  sendOpenCrabsTurn,
  steerOpenCrabsTurn,
  stopOpenCrabsSession,
} from "./opencrabs";
import {
  generateOpenCrabsBranchName,
  generateOpenCrabsCommitMessage,
  generateOpenCrabsPrContent,
} from "./opencrabsGit";
import { generateOpenCrabsSessionTitle } from "./opencrabsTitle";
import { registerHarness, type HarnessAdapter } from "./registry";

export const openCrabsAdapter: HarnessAdapter = {
  id: "opencrabs",
  live: true,
  sendTurn: sendOpenCrabsTurn,
  steerTurn: steerOpenCrabsTurn,
  cancelTurn: cancelOpenCrabsTurn,
  respondApproval: respondOpenCrabsApproval,
  stopSession: stopOpenCrabsSession,
  forgetSession: forgetOpenCrabsSession,
  bindSession: bindOpenCrabsSession,
  generateTitle: generateOpenCrabsSessionTitle,
  generateCommitMessage: generateOpenCrabsCommitMessage,
  generatePrContent: generateOpenCrabsPrContent,
  generateBranchName: generateOpenCrabsBranchName,
};

let registered = false;

export function ensureOpenCrabsRegistered(): void {
  if (registered) return;
  registerHarness(openCrabsAdapter);
  registered = true;
}
