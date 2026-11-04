import { useCallback, useEffect, useRef, useState } from "react";
import {
  loginHarness,
  submitHarnessLoginCode,
} from "../../../integrations/harness/core/auth";
import { copyText } from "../../../platform/tauri/clipboard";
import { HARNESS_TITLE, type HarnessId } from "../model/session";
import { Modal } from "../../../shared/ui/Modal";
import {
  ProviderSignInPanel,
  type ProviderSignInState,
} from "./ProviderSignInPanel";

type Props = {
  harness: HarnessId;
  providerAccountId?: string;
  onClose: () => void;
};

export function ProviderSignInDialog({
  harness,
  providerAccountId,
  onClose,
}: Props) {
  const [state, setState] = useState<ProviderSignInState>("idle");
  const [error, setError] = useState<string | null>(null);
  const [loginUrl, setLoginUrl] = useState<string | null>(null);
  const attempt = useRef<AbortController | null>(null);

  useEffect(() => {
    setState("idle");
    setError(null);
    setLoginUrl(null);
    return () => attempt.current?.abort();
  }, [harness, providerAccountId]);

  const signIn = useCallback(() => {
    const controller = new AbortController();
    attempt.current = controller;
    setState("running");
    setError(null);
    setLoginUrl(null);
    void loginHarness(harness, providerAccountId, {
      signal: controller.signal,
      onUrl: setLoginUrl,
    }).then(
      () => {
        if (attempt.current === controller) {
          attempt.current = null;
          setState("complete");
        }
      },
      (reason: unknown) => {
        if (attempt.current !== controller) return;
        if (controller.signal.aborted) {
          setState("idle");
          setLoginUrl(null);
          return;
        }
        attempt.current = null;
        setState("error");
        setError(
          reason instanceof Error
            ? reason.message
            : `Could not sign in to ${HARNESS_TITLE[harness]}.`,
        );
      },
    );
  }, [harness, providerAccountId]);

  const cancel = useCallback(() => {
    if (!attempt.current) return;
    setState("cancelling");
    attempt.current.abort();
  }, []);

  const close = useCallback(() => {
    attempt.current?.abort();
    onClose();
  }, [onClose]);

  return (
    <Modal
      onClose={close}
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
        onComplete={close}
        onCancel={state === "running" ? cancel : undefined}
        loginUrl={loginUrl}
        onCopyUrl={
          loginUrl
            ? () => {
                const controller = attempt.current;
                if (!controller || controller.signal.aborted) return;
                void copyText(loginUrl).catch(() => {
                  if (
                    attempt.current === controller &&
                    !controller.signal.aborted
                  )
                    setError("Could not copy sign-in link.");
                });
              }
            : undefined
        }
        onSubmitCode={
          loginUrl
            ? (code) => {
                const controller = attempt.current;
                if (!controller || controller.signal.aborted) return;
                void submitHarnessLoginCode(
                  harness,
                  code,
                  controller.signal,
                  providerAccountId,
                ).catch(() => {
                  if (
                    attempt.current === controller &&
                    !controller.signal.aborted
                  )
                    setError("Could not submit sign-in code.");
                });
              }
            : undefined
        }
        completeActionLabel="Continue"
        autoFocus
      />
    </Modal>
  );
}
