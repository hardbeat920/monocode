import { useCallback, useRef, useState } from "react";
import { navigateMono } from "../model/monoNavigation";

export function useMonoSettingsNavigation() {
  const [monoSettingsFor, setMonoSettingsFor] = useState<string | null>(null);
  const [monoSettingsSection, setMonoSettingsSection] = useState<
    "general" | "soul"
  >("general");
  const leaveMonoSettings = useRef<() => Promise<boolean>>(null);
  const monoViewRequest = useRef(0);
  const leave = () => leaveMonoSettings.current?.() ?? Promise.resolve(true);
  const closeMonoSettings = useCallback(() => {
    monoViewRequest.current++;
    setMonoSettingsFor(null);
  }, []);
  const openMono = useCallback(
    <T>(
      id: string,
      section: "general" | "soul" | undefined,
      load: () => Promise<T | undefined>,
      show: (value: T) => void,
    ) =>
      navigateMono(monoViewRequest, leave, load, (value) => {
        if (section) setMonoSettingsSection(section);
        setMonoSettingsFor(section ? id : null);
        show(value);
      }),
    [],
  );
  const leaveMonoForAction = useCallback((action: () => void) => {
    if (!leaveMonoSettings.current) {
      action();
      return;
    }
    void navigateMono(
      monoViewRequest,
      leave,
      async () => true,
      () => {
        setMonoSettingsFor(null);
        action();
      },
    );
  }, []);
  return {
    monoSettingsFor,
    setMonoSettingsFor,
    closeMonoSettings,
    monoSettingsSection,
    leaveMonoSettings,
    monoViewRequest,
    openMono,
    leaveMonoForAction,
  };
}
