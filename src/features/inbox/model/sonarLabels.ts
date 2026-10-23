const LABELS: Record<string, string> = {
  CODE_SMELL: "Code smell",
  BUG: "Bug",
  VULNERABILITY: "Vulnerability",
  SECURITY_HOTSPOT: "Security hotspot",
  BLOCKER: "Blocker",
  CRITICAL: "Critical",
  MAJOR: "Major",
  MINOR: "Minor",
  INFO: "Info",
  HIGH: "High",
  MEDIUM: "Medium",
  LOW: "Low",
  OPEN: "Open",
  CONFIRMED: "Confirmed",
  REOPENED: "Reopened",
  RESOLVED: "Resolved",
  CLOSED: "Closed",
  FIXED: "Fixed",
  ACCEPTED: "Accepted",
  FALSE_POSITIVE: "False positive",
  WONTFIX: "Won’t fix",
  TO_REVIEW: "To review",
  REVIEWED: "Reviewed",
};
export function sonarLabel(value: string | undefined): string {
  if (!value) return "";
  const normalized = value.trim().toUpperCase();
  return (
    LABELS[normalized] ??
    normalized
      .toLowerCase()
      .replace(/_/g, " ")
      .replace(/^\w/, (char) => char.toUpperCase())
  );
}
const TRANSITIONS: Record<string, string> = {
  accept: "Accept",
  falsepositive: "False positive",
  confirm: "Confirm",
  unconfirm: "Unconfirm",
  reopen: "Reopen",
  resolve: "Resolve",
  close: "Close",
  wontfix: "Won’t fix",
  defer: "Defer",
  resolveasreviewed: "Mark reviewed",
  resolveassafe: "Mark safe",
  resolveasacknowledged: "Acknowledge",
  resetastoreview: "Reset to review",
};
export const sonarTransitionLabel = (value: string) =>
  TRANSITIONS[value] ?? sonarLabel(value);
