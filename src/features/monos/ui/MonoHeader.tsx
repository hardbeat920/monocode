import {
  monoProjectsPhrase,
  type MonoLook,
  type MonoState,
} from "../model/mono";
import { PixelMascot } from "../../projects/ui/PixelMascot";
import { MonoStatus } from "./MonoStatus";

type Props = {
  agent: MonoLook;
  state?: MonoState;
  /** Before the first message the header fills the pane and says hello. */
  greeting?: boolean;
};

/**
 * Heads the Mono's conversation: the mascot stands on a name pill,
 * like a messaging app's group header, so the thread reads as a chat with
 * someone rather than another session.
 */
export function MonoHeader({
  agent,
  state = { status: "idle" },
  greeting = false,
}: Props) {
  return (
    <header
      className={`flex flex-col items-center px-6 text-center font-sans ${
        greeting ? "min-h-full justify-center py-12" : "pb-6 pt-8"
      }`}
    >
      <PixelMascot
        name={agent.mascot}
        color={agent.color}
        status={state.status}
        className={`relative z-10 shrink-0 ${greeting ? "size-14" : "size-8"}`}
      />
      <div className="mono-pill -mt-2 flex h-8 items-center rounded-full border px-3.5 backdrop-blur-md">
        <h2 className="text-[13px] font-semibold leading-none text-content">
          {agent.name}
        </h2>
      </div>
      {greeting ? (
        <p className="mt-3 max-w-sm text-[13px] leading-relaxed text-content/50">
          {agent.projects.length
            ? `I can see every session, note and folder in ${monoProjectsPhrase(agent.projects)}. Ask what’s going on, or hand me work.`
            : "Add the projects I should work on from my details, or just ask me something."}
        </p>
      ) : (
        <MonoStatus
          state={state}
          color={agent.color}
          className="mt-2.5 max-w-sm justify-center text-[12px] text-content/45"
        />
      )}
    </header>
  );
}
