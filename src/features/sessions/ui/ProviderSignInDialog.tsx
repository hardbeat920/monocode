import { useCallback, useEffect, useState } from "react";
import { loginHarness } from "../../../integrations/harness/core/auth";
import { HARNESS_TITLE, type HarnessId } from "../model/session";
import { Modal } from "../../../shared/ui/Modal";
import {
  ProviderSignInPanel,
  type ProviderSignInState,
} from "./ProviderSignInPanel";

type Props = {
  harness: HarnessId;
  cwd?: string;
  onClose: () => void;
};

export function ProviderSignInDialog({ harness, cwd, onClose }: Props) {
  const [state, setState] = useState<ProviderSignInState>("idle");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setState("idle");
    setError(null);
  }, [harness]);

  const signIn = useCallback(() => {
    setState("running");
    setError(null);
    void loginHarness(harness, undefined, cwd).then(
      () => setState("complete"),
      (reason: unknown) => {
        setState("error");
        setError(
          reason instanceof Error
            ? reason.message
            : `Could not sign in to ${HARNESS_TITLE[harness]}.`,
        );
      },
    );
  }, [harness, cwd]);

  return (
    <Modal
      onClose={onClose}
      title="Authentication required"
      description={`Sign in to continue using ${HARNESS_TITLE[harness]}.`}
      size="sm"
      minimalHeader
    >
      <ProviderSignInPanel
        harness={harness}
        state={state}
        error={error}
        onSignIn={signIn}
        onComplete={onClose}
        completeActionLabel="Continue"
        autoFocus
      />
    </Modal>
  );
}
