import { useLocalSearchParams } from "expo-router";

import { DocumentScreen } from "@/features/files/DocumentScreen";

// A document from a Mono's profile, pushed inside the profile's sheet.
export default function ProfileDocumentRoute() {
  const { documentId, title } = useLocalSearchParams<{ documentId: string; title?: string }>();
  return <DocumentScreen documentId={documentId} title={title} stacked />;
}
