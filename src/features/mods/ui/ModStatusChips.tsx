import { iconByName } from "../../../shared/ui/icons";
import { useModHost } from "../runtime/host";
import { toneColor } from "./ModElement";

/** `$.ui.status` chips, in the session footer beside the usage chips. */
export function ModStatusChips() {
  const host = useModHost();
  return (
    <>
      {host.status().map((chip) => {
        const Icon = iconByName(chip.icon);
        return (
          <span
            key={`${chip.name}:${chip.id}`}
            title={`${chip.name}: ${chip.text}`}
            style={{ color: toneColor(chip.tone) }}
            className="inline-flex min-w-0 shrink-0 items-center gap-1.5 whitespace-nowrap"
          >
            {Icon ? (
              <Icon className="size-3 shrink-0" strokeWidth={1.75} aria-hidden />
            ) : null}
            <span>{chip.text}</span>
          </span>
        );
      })}
    </>
  );
}
