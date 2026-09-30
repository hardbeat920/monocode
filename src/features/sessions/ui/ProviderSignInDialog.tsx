import { useCallback, useEffect, useState } from "react";
import { loginHarness } from "../../../integrations/harness/core/auth";
import { HARNESS_TITLE, type HarnessId } from "../model/session";
import { Modal } from "../../../shared/ui/Modal";
import {
  ProviderSignInPanel,
  type ProviderSignInState,
} from "./ProviderSignInPanel";
import { useTranslation } from "../../i18n/model/i18n";

type Props = {
  harness: HarnessId;
  onClose: () => void;
};

export function ProviderSignInDialog({ harness, onClose }: Props) {
  const { t } = useTranslation();
  const [state, setState] = useState<ProviderSignInState>("idle");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setState("idle");
    setError(null);
  }, [harness]);

  const providerName = HARNESS_TITLE[harness];

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
            : t("dialogs.providerSignInFailed", `Could not sign in to ${providerName}.`, {
                provider: providerName,
              }),
        );
      },
    );
  }, [harness, providerName, t]);

  return (
    <Modal
      onClose={onClose}
      title={t("dialogs.providerSignInTitle", "Authentication required")}
      description={t(
        "dialogs.providerSignInDescription",
        `Sign in to continue using ${providerName}.`,
        { provider: providerName },
      )}
      size="sm"
      minimalHeader
    >
      <ProviderSignInPanel
        harness={harness}
        state={state}
        error={error}
        onSignIn={signIn}
        onComplete={onClose}
        completeActionLabel={t("dialogs.providerSignInContinue", "Continue")}
        autoFocus
      />
    </Modal>
  );
}
