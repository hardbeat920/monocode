/** Settings page building blocks: a titled card (`Group`) of label/control rows (`Row`). */
import { createContext, useContext, type ReactNode } from "react";

export const settingDomId = (id: string) => `setting-${id}`;

/** The row or group Settings just jumped to, so it can flash where you landed. */
export const RevealedSetting = createContext<string | null>(null);

/**
 * A titled card of related settings. Everything on a page lives in one, so a
 * page reads as a handful of topics instead of one long list of switches.
 */
export function Group({
  id,
  title,
  description,
  action,
  children,
}: {
  /** Matches a `SETTINGS_INDEX` id when the whole card is the search target. */
  id?: string;
  title: ReactNode;
  description?: string;
  action?: ReactNode;
  children: ReactNode;
}) {
  const revealed = useContext(RevealedSetting);
  const flash = id != null && revealed === id;

  return (
    <section
      id={id ? settingDomId(id) : undefined}
      data-setting-id={id}
      className="pt-8 first:pt-0"
    >
      <div className="flex items-end gap-4 pb-2.5">
        <div className="min-w-0 flex-1">
          <h2 className="text-[13px] font-semibold text-content">{title}</h2>
          {description ? (
            <p className="mt-1 text-[12px] leading-relaxed text-content/45">
              {description}
            </p>
          ) : null}
        </div>
        {action ? <div className="shrink-0 pb-0.5">{action}</div> : null}
      </div>
      <div
        className={`overflow-hidden rounded-xl border bg-content/3 transition-colors ${
          flash ? "border-accent/60" : "border-content/10"
        }`}
      >
        {children}
      </div>
    </section>
  );
}

export function Row({
  id,
  label,
  description,
  children,
}: {
  /** Matches a `SETTINGS_INDEX` id so search can scroll here. */
  id?: string;
  label: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
}) {
  const revealed = useContext(RevealedSetting);
  const flash = id != null && revealed === id;

  return (
    <div
      id={id ? settingDomId(id) : undefined}
      data-setting-id={id}
      className={`settings-row flex items-start gap-6 border-b border-content/5 px-4 py-3.5 transition-colors last:border-b-0 ${
        flash ? "bg-accent/10" : ""
      }`}
    >
      <div className="min-w-0 flex-1">
        <div className="text-[13px] font-medium text-content">{label}</div>
        {description ? (
          <p className="mt-1 text-[12px] leading-relaxed text-content/45">
            {description}
          </p>
        ) : null}
      </div>
      <div className="settings-row-control flex min-w-0 max-w-[60%] shrink-0 flex-wrap items-center justify-end gap-2">
        {children}
      </div>
    </div>
  );
}
