import { Redirect, useLocalSearchParams } from "expo-router";

import { AgentsSheet } from "@/features/orchestration/AgentsSheet";

export default function AgentsRoute() {
  const { projectId, sessionId } = useLocalSearchParams<{
    projectId: string;
    sessionId: string;
  }>();
  if (!projectId || !sessionId) return <Redirect href="/" />;
  return <AgentsSheet projectId={projectId} sessionId={sessionId} />;
}
