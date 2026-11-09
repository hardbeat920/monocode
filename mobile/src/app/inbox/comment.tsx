import { Redirect, useLocalSearchParams } from "expo-router";

import { CommentSheet } from "@/features/inbox/CommentSheet";

export default function InboxCommentRoute() {
  const { key, replyTo, author } = useLocalSearchParams<{
    key: string;
    replyTo?: string;
    author?: string;
  }>();
  if (!key) return <Redirect href="/" />;
  return <CommentSheet itemKey={key} replyTo={replyTo} author={author} />;
}
