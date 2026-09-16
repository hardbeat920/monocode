import { useCallback, useEffect, useState } from "react";
import { loginHarness } from "../lib/harness/auth";
import { HARNESS_TITLE, type HarnessId } from "../lib/session";
import { Modal } from "./Modal";
import {
  ProviderSignInPanel,
  type ProviderSignInState,
} from "./ProviderSignInPanel";

type Props = {
  harness: HarnessId;
  onClose: () => void;
};

export function ProviderSignInDialog({ harness, onClose }: Props) {
  const [state, setState] = useState<ProviderSignInState>("idle");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setState("idle");
    setError(null);
  }, [harness]);

  const signIn = useCallback(() => {
    setState("running");
    setError(null);
    void loginHarness(harness).then(
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
  }, [harness]);

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
