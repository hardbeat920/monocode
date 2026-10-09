import { useCallback, useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { SecondaryButton } from "../../../shared/ui/SecondaryButton";
import { loginHarness } from "../../../integrations/harness/core/auth";
import {
  cancelNativeAntigravityLogin,
  logoutNativeAntigravity,
  nativeAntigravityAccount,
  refreshNativeAntigravityCatalog,
  type AntigravityAccountStatus,
} from "../../../integrations/harness/providers/antigravity/antigravityNative";

/** Windows account actions replace the Antigravity executable-path control. */
export function AntigravityAccountControl({ cwd }: { cwd?: string }) {
  const [account, setAccount] = useState<AntigravityAccountStatus>();
  const [working, setWorking] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string>();
  const refreshStatus = useCallback(async () => {
    try {
      const next = await nativeAntigravityAccount();
      setAccount(next);
      if (next.authError) setError(next.authError);
    }
    catch (error) { setError(error instanceof Error ? error.message : String(error)); }
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void refreshStatus();
    void getCurrentWindow().listen("antigravity-native-account-changed", () => {
      if (!disposed) void refreshStatus();
    }).then((stop) => { if (disposed) stop(); else unlisten = stop; })
      .catch((error: Error) => { if (!disposed) setError(error.message); });
    return () => { disposed = true; unlisten?.(); };
  }, [refreshStatus]);

  async function run(action: () => Promise<void>, login = false) {
    setWorking(true); setConnecting(login); setError(undefined);
    try { await action(); }
    catch (error) { setError(error instanceof Error ? error.message : String(error)); }
    finally { await refreshStatus(); setWorking(false); setConnecting(false); }
  }

  return (
    <span className="flex flex-wrap items-center gap-2 font-normal" aria-label="Antigravity Google account">
      <span className="text-[11px] text-content/50">
        {account?.authenticated ? account.email : account ? "Not signed in" : "Checking account…"}
      </span>
      {account?.authenticated ? (
        <>
          <SecondaryButton disabled={working}
            onClick={() => void run(logoutNativeAntigravity)}>Disconnect Google</SecondaryButton>
          <SecondaryButton disabled={working}
            onClick={() => void run(refreshNativeAntigravityCatalog)}>Refresh models</SecondaryButton>
        </>
      ) : (
        <SecondaryButton
          disabled={working || account?.loginPending || !account?.backendAvailable}
          onClick={() => void run(() => loginHarness("antigravity", undefined, cwd), true)}>
          {connecting ? "Connecting…" : "Connect Google"}
        </SecondaryButton>
      )}
      {connecting ? (
        <SecondaryButton
          onClick={() => void cancelNativeAntigravityLogin().catch((error: Error) => setError(error.message))}>
          Cancel sign-in
        </SecondaryButton>
      ) : null}
      {error ? <span role="alert" className="basis-full text-[11px] text-red-400">{error}</span> : null}
    </span>
  );
}
