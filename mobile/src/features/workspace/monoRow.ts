import { plainText } from "@/features/sessions/markdownParser";

import type { Mono } from "./types";

/** When a Mono was last active, as a chat list dates its rows: the time
 *  today, Yesterday, else the day. */
export function when(at: number, now = new Date()) {
  const date = new Date(at);
  if (date.toDateString() === now.toDateString())
    return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  if (date.toDateString() === yesterday.toDateString()) return "Yesterday";
  return date.toLocaleDateString([], { month: "short", day: "numeric" });
}

/** The start of a message as one plain line: its markdown read for its
 *  words, so **bold** shows as bold rather than its asterisks. Only as much
 *  as one line can show is read. */
export function oneLine(text: string) {
  // A reply that is only a reaction, as the chat shows it, by its emoji.
  const reaction = text.trim().match(/^<reaction>\s*(.{1,16}?)\s*<\/reaction>$/su);
  if (reaction) return `Reacted ${reaction[1]}`;
  return plainText(text.slice(0, 600));
}

/** What a Mono's row says under its name: what it is doing, else its last line. */
export function monoDetail(mono: Pick<Mono, "status" | "lastMessage">) {
  if (mono.status === "working") return "Working…";
  if (mono.status === "needs-you") return "Needs you";
  return oneLine(mono.lastMessage) || "No messages yet";
}
