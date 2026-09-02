import { useEffect, useState } from "react";
import { loadRemoteToken, rememberRemoteToken } from "../bridge/remoteClient";
import { isRemoteSession } from "../bridge/remoteClient";

export function RemoteGate({ children }: { children: React.ReactNode }) {
  const [ready, setReady] = useState(!isRemoteSession());
  const [token, setToken] = useState("");

  useEffect(() => {
    if (!isRemoteSession()) return;
    const existing = loadRemoteToken();
    if (existing) {
      setReady(true);
      return;
    }
    setReady(false);
  }, []);

  if (!isRemoteSession()) {
    return children;
  }

  if (ready) {
    return children;
  }

  const onSubmit = (event: React.FormEvent) => {
    event.preventDefault();
    const trimmed = token.trim();
    if (!trimmed) return;
    rememberRemoteToken(trimmed);
    setReady(true);
  };

  return (
    <div className="flex min-h-screen items-center justify-center bg-[#171717] px-6 text-content">
      <form
        onSubmit={onSubmit}
        className="w-full max-w-md rounded-2xl border border-content/10 bg-content/[0.03] p-6 shadow-2xl"
      >
        <h1 className="text-[18px] font-medium">Connect to MonoCode</h1>
        <p className="mt-2 text-[13px] leading-relaxed text-content/55">
          Enter the remote access token shown in MonoCode settings on the host
          machine. Use the same token for LAN or Tailscale access.
        </p>
        <label className="mt-5 block text-[12px] text-content/45">
          Access token
          <input
            autoFocus
            value={token}
            onChange={(event) => setToken(event.target.value)}
            className="mt-1.5 w-full rounded-lg border border-content/10 bg-content/5 px-3 py-2 text-[13px] text-content outline-none focus:border-content/25"
            placeholder="Paste token from desktop settings"
            spellCheck={false}
          />
        </label>
        <button
          type="submit"
          className="mt-5 w-full rounded-lg bg-content px-3 py-2 text-[13px] font-medium text-[#171717] hover:opacity-90"
        >
          Connect
        </button>
      </form>
    </div>
  );
}
