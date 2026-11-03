import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import QRCode from "qrcode";
import { Plus, SmartPhone, Trash2 } from "../../../shared/ui/icons";
import { PairPhoneDialog } from "./PairPhoneDialog";

type Device = { id: string; name: string; fingerprint: string };
type Status = {
  enabled: boolean;
  listening: boolean;
  name: string;
  fingerprint: string | null;
  openRouter: boolean;
  remoteAddress: string;
  routerStatus: string;
  addresses: { host: string; port: number }[];
  devices: Device[];
};
type Invite = { url: string; expires: number };

const input =
  "w-full rounded-lg border border-content/15 bg-content/3 px-3 py-2 text-[13px] outline-none focus:border-content/35";
const button =
  "rounded-lg bg-selection px-3 py-2 text-[13px] font-medium hover:bg-selection-hover disabled:opacity-40";
const quiet =
  "px-3 py-2 text-[13px] text-content/50 hover:text-content disabled:opacity-40";

export function MobileSettings() {
  const [status, setStatus] = useState<Status | null>(null);
  const [remote, setRemote] = useState("");
  const [invite, setInvite] = useState<Invite | null>(null);
  const [image, setImage] = useState("");
  const [now, setNow] = useState(() => Date.now() / 1000);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const [removing, setRemoving] = useState<string>();
  /** Device ids already paired when the visible code was made. */
  const known = useRef<Set<string> | null>(null);

  useEffect(() => {
    let disposed = false;
    let initial = true;
    const load = async () => {
      try {
        const next = await invoke<Status>("mobile_status");
        if (!next) throw new Error("Desktop mobile service is unavailable");
        if (!disposed) {
          if (initial) setRemote(next.remoteAddress);
          initial = false;
          setStatus(next);
        }
      } catch (reason) {
        if (!disposed) setError(String(reason));
      }
    };
    void load();
    const timer = setInterval(() => void load(), 2000);
    return () => {
      disposed = true;
      clearInterval(timer);
      void invoke("mobile_cancel_invite");
    };
  }, []);

  // A phone that appears while the code is showing just paired with it.
  useEffect(() => {
    if (!invite || !status || !known.current) return;
    const paired = status.devices.find(
      (device) => !known.current!.has(device.id),
    );
    if (!paired) return;
    known.current = null;
    setInvite(null);
    setNotice(`${paired.name} is paired.`);
    void invoke("mobile_cancel_invite");
  }, [invite, status]);

  useEffect(() => {
    let disposed = false;
    if (!invite) {
      setImage("");
      return;
    }
    void QRCode.toDataURL(invite.url, {
      width: 720,
      margin: 2,
      errorCorrectionLevel: "L",
    })
      .then((value) => {
        if (!disposed) setImage(value);
      })
      .catch((reason) => setError(String(reason)));
    return () => {
      disposed = true;
    };
  }, [invite]);

  useEffect(() => {
    if (!invite) return;
    setNow(Date.now() / 1000);
    const timer = setInterval(() => setNow(Date.now() / 1000), 1000);
    return () => clearInterval(timer);
  }, [invite]);

  const act = async (action: () => Promise<unknown>) => {
    setBusy(true);
    setError("");
    setNotice("");
    try {
      await action();
      setStatus(await invoke<Status>("mobile_status"));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };
  const configure = (enabled: boolean, openRouter: boolean) =>
    invoke("mobile_configure", { enabled, openRouter, remoteAddress: remote });
  // The desktop only listens while a phone is paired or being paired.
  const stopListening = () => configure(false, status?.openRouter ?? false);
  const pair = () =>
    act(async () => {
      if (!status?.enabled || !status.listening)
        await configure(true, status?.openRouter ?? false);
      known.current = new Set(status?.devices.map((device) => device.id));
      setInvite(await invoke<Invite>("mobile_invite"));
    });
  const closeInvite = () =>
    act(async () => {
      await invoke("mobile_cancel_invite");
      known.current = null;
      setInvite(null);
      if (!status?.devices.length) await stopListening();
    });

  const devices = status?.devices ?? [];
  const left = invite ? Math.max(0, Math.floor(invite.expires - now)) : 0;

  return (
    <div
      data-setting-id="mobile-access"
      className="mt-10 flex flex-col gap-5 border-t border-stroke pt-8"
    >
      <div className="flex items-end justify-between gap-4">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <h2 className="text-[13px] font-semibold text-content">
              Your phones
            </h2>
            <span className="rounded-md border border-content/10 bg-content/5 px-1.5 py-0.5 text-[10px] font-medium text-content/60">
              Invite-only beta
            </span>
          </div>
          <p className="mt-1 text-[12px] leading-relaxed text-content/45">
            Check on projects and conversations from the MonoCode mobile
            app. Your phone connects straight to this desktop.
          </p>
          <p className="mt-1 text-[12px] leading-relaxed text-content/45">
            The iPhone app currently requires a TestFlight invitation.
          </p>
        </div>
        <button
          className={`${button} flex shrink-0 items-center gap-2`}
          disabled={!status || busy || invite !== null}
          onClick={() => void pair()}
        >
          <Plus className="size-4" /> Pair phone
        </button>
      </div>

      {invite && (
        <PairPhoneDialog
          image={image}
          url={invite.url}
          left={left}
          busy={busy}
          onNewCode={() => void pair()}
          onError={setError}
          onClose={() => void closeInvite()}
        />
      )}

      {status && devices.length > 0 && !status.listening ? (
        <div className="flex items-center justify-between gap-4 rounded-lg bg-amber-500/5 px-4 py-3 text-[12px] text-amber-500">
          Your phones can’t connect because this desktop isn’t accepting
          connections right now.
          <button
            className={`${button} shrink-0 text-content`}
            disabled={busy}
            onClick={() => void act(() => configure(true, status.openRouter))}
          >
            Reconnect
          </button>
        </div>
      ) : null}

      {devices.length > 0 ? (
        <div className="divide-y divide-stroke overflow-hidden rounded-xl border border-stroke">
          {devices.map((device) => (
            <div key={device.id}>
              <div className="flex items-center gap-3 px-4 py-4">
                <SmartPhone className="size-5 shrink-0 text-content/45" />
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[13px] font-medium">
                    {device.name}
                  </div>
                  <div className="mt-1 text-[12px] text-content/50">Paired</div>
                </div>
                <button
                  disabled={busy}
                  className="rounded p-2 text-content/40 hover:bg-selection hover:text-content disabled:opacity-40"
                  aria-label={`Remove ${device.name}`}
                  title="Remove phone…"
                  onClick={() => {
                    setError("");
                    setRemoving(device.id);
                  }}
                >
                  <Trash2 className="size-4" />
                </button>
              </div>
              {removing === device.id && (
                <div
                  role="group"
                  aria-label={`Confirm removing ${device.name}`}
                  className="flex flex-col gap-3 border-t border-stroke bg-content/3 px-4 py-4 text-[12px] leading-relaxed text-content/60"
                >
                  <p className="text-[13px] font-medium text-content">
                    Remove {device.name}?
                  </p>
                  <p>
                    It disconnects right away and can’t reach this desktop again
                    until you pair it with a new code.
                  </p>
                  <div className="flex gap-2">
                    <button
                      className={button}
                      disabled={busy}
                      onClick={() =>
                        void act(async () => {
                          await invoke("mobile_revoke", {
                            deviceId: device.id,
                          });
                          if (devices.length === 1 && !invite)
                            await stopListening();
                          setRemoving(undefined);
                          setNotice(`${device.name} was removed.`);
                        })
                      }
                    >
                      Remove phone
                    </button>
                    <button
                      className={quiet}
                      disabled={busy}
                      onClick={() => setRemoving(undefined)}
                    >
                      Cancel
                    </button>
                  </div>
                </div>
              )}
            </div>
          ))}
        </div>
      ) : status ? (
        <div className="rounded-xl border border-dashed border-content/15 px-5 py-8 text-center text-[13px] text-content/45">
          Pair your iPhone or Android phone to get started.
        </div>
      ) : null}

      {error && (
        <p
          role="alert"
          className="whitespace-pre-wrap break-words rounded-lg bg-red-500/5 p-3 text-[12px] leading-relaxed text-red-400"
        >
          {error}
        </p>
      )}
      {notice && (
        <p role="status" className="text-[13px] text-emerald-500">
          {notice}
        </p>
      )}

      {status ? (
        <details className="text-[12px] text-content/45">
          <summary className="cursor-pointer">Connect away from home</summary>
          <div className="mt-4 flex flex-col gap-3 leading-relaxed">
            <p>
              On the same Wi-Fi, phones connect without any setup. To reach this
              desktop from anywhere else, it needs a public address. Keep the
              desktop awake.
            </p>
            <label className="flex items-center gap-2 text-[13px] text-content/70">
              <input
                type="checkbox"
                checked={status.openRouter}
                disabled={busy}
                onChange={(event) =>
                  void act(() =>
                    configure(status.enabled, event.target.checked),
                  )
                }
              />
              Open the router port automatically
            </label>
            <p>{status.routerStatus}</p>
            <div className="flex flex-col gap-1.5 text-content/65">
              Remote address
              <div className="flex gap-2">
                <input
                  type="text"
                  aria-label="Remote address"
                  className={input}
                  value={remote}
                  onChange={(event) => setRemote(event.target.value)}
                  placeholder="Public hostname or VPN IP, optionally :port"
                  autoCapitalize="none"
                  spellCheck={false}
                />
                <button
                  className={`${button} shrink-0`}
                  disabled={busy || remote.trim() === status.remoteAddress}
                  onClick={() =>
                    void act(() => configure(status.enabled, status.openRouter))
                  }
                >
                  Save
                </button>
              </div>
            </div>
            <p>
              Automatic mapping needs a public IP. Otherwise, forward UDP port
              38416 to this desktop or use a VPN address. Set the address before
              making a pairing code.
            </p>
          </div>
        </details>
      ) : null}
    </div>
  );
}
