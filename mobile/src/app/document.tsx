import { useLocalSearchParams } from "expo-router";

import { DocumentScreen } from "@/features/files/DocumentScreen";

// A document a Mono wrote, presented as a native modal (see the root layout).
export default function DocumentRoute() {
  const { documentId, title } = useLocalSearchParams<{ documentId: string; title?: string }>();
  return <DocumentScreen documentId={documentId} title={title} />;
}
