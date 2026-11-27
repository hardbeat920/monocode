import { useLocalSearchParams } from "expo-router";

import { ModelOptions } from "@/features/models/ModelPicker";
import { monoConversationId } from "@/features/workspace/WorkspaceProvider";

// The Mono's model options and access, pushed from its model list.
export default function MonoModelOptionsScreen() {
  const { monoId } = useLocalSearchParams<{ monoId: string }>();
  return <ModelOptions projectId="" sessionId={monoConversationId(monoId)} />;
}
