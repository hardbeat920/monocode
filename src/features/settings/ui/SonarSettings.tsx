import { useEffect, useState } from "react";
import { SecondaryButton } from "../../../shared/ui/SecondaryButton";
import { clearInboxCache } from "../../inbox/model/githubTasks";
import {
  disconnectSonar,
  saveSonarConfig,
  sonarConnected,
  type SonarStatus,
} from "../../inbox/model/sonar";

export function SonarSettings() {
  const [status, setStatus] = useState<SonarStatus | null>(null);
  const [site, setSite] = useState("https://sonarcloud.io");
  const [keys, setKeys] = useState("");
  const [token, setToken] = useState("");
  const [checking, setChecking] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    void sonarConnected()
      .then((next) => {
        if (!cancelled) setStatus(next);
      })
      .catch((err) => {
        if (!cancelled) setError(String(err));
      })
      .finally(() => {
        if (!cancelled) setChecking(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);
  const connect = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const next = await saveSonarConfig({
        site: site.trim(),
        projectKeys: keys.split(/[\s,]+/).filter(Boolean),
        token: token.trim(),
      });
      setStatus(next);
      setToken("");
      clearInboxCache();
    } catch (err) {
      setError(String(err instanceof Error ? err.message : err));
    } finally {
      setBusy(false);
    }
  };
  const disconnect = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      setStatus(await disconnectSonar());
      setToken("");
      clearInboxCache();
    } catch (err) {
      setError(String(err instanceof Error ? err.message : err));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="px-4 py-3.5">
      {checking ? (
        <p className="text-[12px] text-content/45">
          Checking Sonar connection…
        </p>
      ) : status?.connected ? (
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="min-w-0 text-[12px] text-content/65">
            <p className="break-all">{status.site}</p>
            <p className="break-all">{status.projectKeys.join(", ")}</p>
          </div>
          <SecondaryButton disabled={busy} onClick={() => void disconnect()}>
            {busy ? "Disconnecting" : "Disconnect"}
          </SecondaryButton>
        </div>
      ) : (
        <form
          onSubmit={(event) => {
            event.preventDefault();
            void connect();
          }}
          className="flex flex-col gap-3"
        >
          <p className="text-[12px] leading-relaxed text-content/45">
            Use a personal access token with Browse permission. Project keys are
            the sonar.projectKey values from your analysis setup. Separate
            multiple keys with commas. Disconnect deletes the saved credentials.
          </p>
          {(
            [
              {
                label: "Sonar URL",
                value: site,
                set: setSite,
                type: "text",
                placeholder: "https://sonarcloud.io",
              },
              {
                label: "Sonar project keys",
                value: keys,
                set: setKeys,
                type: "text",
                placeholder: "organization_project",
              },
              {
                label: "Sonar personal access token",
                value: token,
                set: setToken,
                type: "password",
                placeholder: "Personal access token",
              },
            ] as const
          ).map((field) => (
            <label
              key={field.label}
              className="flex flex-col gap-1 text-[12px] text-content/65"
            >
              {field.label}
              <input
                aria-label={field.label}
                value={field.value}
                type={field.type}
                onChange={(event) => field.set(event.target.value)}
                placeholder={field.placeholder}
                required
                disabled={busy}
                autoComplete="off"
                spellCheck={false}
                className="h-8 w-full rounded-md border border-content/10 bg-transparent px-2 text-content outline-none focus:border-content/20"
              />
            </label>
          ))}
          <p className="text-[11px] text-content/45">
            For US SonarQube Cloud, use https://sonarqube.us. Self-hosted
            SonarQube URLs are also supported.
          </p>
          <div>
            <SecondaryButton
              type="submit"
              disabled={busy || !site.trim() || !keys.trim() || !token.trim()}
            >
              {busy ? "Connecting…" : "Connect Sonar"}
            </SecondaryButton>
          </div>
        </form>
      )}
      {error ? (
        <p role="alert" className="mt-3 break-words text-[12px] text-rose-400">
          {error}
        </p>
      ) : null}
    </div>
  );
}
