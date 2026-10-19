import { useCallback, useEffect, useState } from "react";
import { loginHarness } from "../../../integrations/harness/core/auth";
import { HARNESS_TITLE, type HarnessId } from "../model/session";
import { Modal } from "../../../shared/ui/Modal";
import {
  ProviderSignInPanel,
  type ProviderSignInState,
} from "./ProviderSignInPanel";
import { useTranslate } from "../../../shared/i18n/locale";
type Props = {
  harness: HarnessId;
  onClose: () => void;
};

export function ProviderSignInDialog({ harness, onClose }: Props) {
  const t = useTranslate();
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
            : t("Could not sign in to {name}.", { name: HARNESS_TITLE[harness] }),
        );
      },
    );
  }, [harness]);

  return (
    <Modal
      onClose={onClose}
      title={t("Authentication required")}
      description={t("Sign in to continue using {name}.", {
        name: HARNESS_TITLE[harness],
      })}
      size="sm"
      minimalHeader
    >
      <ProviderSignInPanel
        harness={harness}
        state={state}
        error={error}
        onSignIn={signIn}
        onComplete={onClose}
        completeActionLabel={t("Continue")}
        autoFocus
      />
    </Modal>
  );
}
