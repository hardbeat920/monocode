import { useState, type ReactNode } from "react";
import {
  rememberedMachine,
  rememberMachine,
  useRemoteMachines,
} from "../model/connections";
import { MachinePicker } from "./MachinePicker";
import { RemoteSessionPane } from "./RemoteSessionPane";

/** Chooses where a new session runs. Local sessions always render the same
 * `local` element, so sending the first message (which makes the session
 * no longer choosable) never remounts the local pane. */
export function SessionMachineRouter({
  cwd,
  choosable,
  local,
}: {
  cwd: string;
  choosable: boolean;
  local: (machineControl?: ReactNode) => ReactNode;
}) {
  const { machines, loaded } = useRemoteMachines(choosable);
  const [choice, setChoice] = useState(() => ({
    cwd,
    machineId: choosable ? rememberedMachine(cwd) : undefined,
  }));
  if (choosable && choice.cwd !== cwd)
    setChoice({ cwd, machineId: rememberedMachine(cwd) });
  const machineId = choosable ? choice.machineId : undefined;
  const picker = choosable ? (
    <MachinePicker
      machines={machines}
      selected={machineId}
      onSelect={(id) => {
        rememberMachine(cwd, id);
        setChoice({ cwd, machineId: id });
      }}
    />
  ) : undefined;
  if (!machineId) return local(picker);
  const machine = machines.find((machine) => machine.id === machineId);
  if (!machine)
    return (
      <div className="flex h-full flex-col items-start gap-4 p-6">
        {picker}
        <p className="text-[13px] text-content/50">
          {loaded
            ? "This machine is no longer connected. Add it again to open its sessions, or choose another machine."
            : "Loading machine…"}
        </p>
      </div>
    );
  return (
    <RemoteSessionPane
      key={`${machine.id}:${cwd}`}
      machine={machine}
      project={cwd}
      machinePicker={picker}
    />
  );
}
