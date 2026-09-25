import { useRef, useState } from "react";
import { ChevronDown, Plus } from "../../../shared/ui/icons";
import { Popover } from "../../../shared/ui/Popover";
import { OPEN_CONNECTIONS_EVENT } from "../model/connections";
import type { RemoteMachine } from "../model/protocol";

export function MachinePicker({
  machines,
  selected,
  onSelect,
}: {
  machines: RemoteMachine[];
  selected?: string;
  onSelect: (id?: string) => void;
}) {
  const anchor = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const label = selected
    ? (machines.find((machine) => machine.id === selected)?.name ??
      "Disconnected machine")
    : "This computer";
  const choose = (id?: string) => {
    onSelect(id);
    setOpen(false);
  };
  return (
    <>
      <button
        ref={anchor}
        type="button"
        aria-label="Choose execution machine"
        onClick={() => setOpen(!open)}
        className="flex shrink-0 items-center gap-1.5 text-[12px] text-content/60 hover:text-content"
      >
        <span className="truncate">{label}</span>
        <ChevronDown className="size-3" />
      </button>
      {open && (
        <Popover
          anchor={anchor}
          align="end"
          width={300}
          onDismiss={() => setOpen(false)}
          className="p-2 text-[12px]"
        >
          <div className="px-2 py-2 text-[11px] text-content/40">
            Run sessions on
          </div>
          <button
            className="w-full rounded-md px-2 py-2 text-left hover:bg-selection"
            onClick={() => choose()}
          >
            This computer
          </button>
          {machines.map((machine) => (
            <button
              key={machine.id}
              className="w-full rounded-md px-2 py-2 text-left hover:bg-selection"
              onClick={() => choose(machine.id)}
            >
              <div className="truncate">{machine.name}</div>
              <div className="truncate text-content/40">
                {machine.ssh?.target ?? machine.endpoint}
              </div>
            </button>
          ))}
          <div className="mt-2 border-t border-stroke pt-2">
            <button
              className="flex w-full items-center gap-2 rounded-md px-2 py-2 text-left text-content/60 hover:bg-selection hover:text-content"
              onClick={() => {
                setOpen(false);
                window.dispatchEvent(new Event(OPEN_CONNECTIONS_EVENT));
              }}
            >
              <Plus className="size-3.5" /> Manage machines in Settings
            </button>
          </div>
        </Popover>
      )}
    </>
  );
}
