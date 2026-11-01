import { Redirect, useLocalSearchParams } from "expo-router";

import { StartAgentSheet } from "@/features/inbox/StartAgentSheet";

export default function InboxStartRoute() {
  const { key, text } = useLocalSearchParams<{ key: string; text?: string }>();
  if (!key) return <Redirect href="/" />;
  return <StartAgentSheet itemKey={key} presetText={text} />;
}
