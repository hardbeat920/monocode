import { router, Stack, useLocalSearchParams } from "expo-router";
import { useEffect } from "react";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { InboxPage, SCOPE_SPACE, useProjectNames } from "@/features/inbox/InboxPage";
import { useInboxActionsSupported, useInboxSupported } from "@/features/inbox/InboxProvider";
import { SheetLayout } from "@/features/sessions/SessionScreen";
import { useNativeHeaderHeight } from "@/ui/useNativeHeaderHeight";

// The Inbox, pushed from its row in the drawer. `projectId` is the project in
// front when it was opened, which heads the page's project picker.
export default function InboxListScreen() {
  const { projectId } = useLocalSearchParams<{ projectId?: string }>();
  const names = useProjectNames();
  const supported = useInboxSupported();
  const canReadAll = useInboxActionsSupported();
  const insets = useSafeAreaInsets();
  const headerHeight = useNativeHeaderHeight();

  // The desktop can drop its Inbox while this is open.
  useEffect(() => {
    if (!supported && router.canGoBack()) router.back();
  }, [supported]);

  return (
    <>
      <Stack.Screen options={{ title: "Inbox" }} />
      <SheetLayout
        top={headerHeight + SCOPE_SPACE}
        // The search rests just above the keyboard, as the drawer's does.
        keyboardOffset={Math.max(insets.bottom, 12) - 10}
      >
        <InboxPage pinned={projectId} names={names} canReadAll={canReadAll} />
      </SheetLayout>
    </>
  );
}
