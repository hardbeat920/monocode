import { Redirect, useLocalSearchParams } from "expo-router";

import { ItemSheet } from "@/features/inbox/ItemSheet";

export default function InboxItemRoute() {
  const { key } = useLocalSearchParams<{ key: string }>();
  if (!key) return <Redirect href="/" />;
  return <ItemSheet itemKey={key} />;
}
