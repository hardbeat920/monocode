import { useLocalSearchParams } from "expo-router";

import { ModelOptions } from "@/features/models/ModelPicker";

// The chosen model's options and the session's access, pushed from the models.
export default function ModelOptionsScreen() {
  const { projectId, sessionId } = useLocalSearchParams<{
    projectId: string;
    sessionId?: string;
  }>();
  return <ModelOptions projectId={projectId} sessionId={sessionId} />;
}
