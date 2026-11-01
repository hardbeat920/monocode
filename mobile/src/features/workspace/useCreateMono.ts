import { router } from "expo-router";
import { useCallback, useRef } from "react";
import { Alert } from "react-native";

import { useConnection, useMonoTools } from "@/features/connection/ConnectionProvider";

/**
 * Makes a Mono on the desktop to work in `projectId`, shows it on that
 * project's Sessions page at once, then opens its chat with its look open to
 * name it. `undefined` where the desktop app is too
 * old to make one from the phone, so callers leave their entry out.
 */
export function useCreateMono(projectId: string) {
  const { command, refresh } = useConnection();
  const tools = useMonoTools();
  const creating = useRef(false);
  const create = useCallback(async () => {
    if (creating.current) return;
    creating.current = true;
    try {
      const { monoId } = await command<{ monoId: string }>("mono.create");
      // A Mono with no project is listed in every one; this one starts in its own.
      await command("mono.update", { monoId, addProject: projectId });
      await refresh();
      router.replace({ pathname: "/mono/[monoId]", params: { monoId } });
      router.push({ pathname: "/mono-profile/[monoId]/edit", params: { monoId } });
    } catch (reason) {
      Alert.alert("Couldn’t create a Mono", reason instanceof Error ? reason.message : String(reason));
    } finally {
      creating.current = false;
    }
  }, [command, refresh, projectId]);
  return tools ? create : undefined;
}
