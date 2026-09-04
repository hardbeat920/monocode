import type { BuiltinSkill } from "./skills";

export const PLAN_COMMAND: BuiltinSkill = {
  kind: "builtin",
  name: "plan",
  invocation: "plan",
  description: "Create a reviewable implementation plan before changing files.",
  scope: "builtin",
  source: "monocode",
};

/** Consume `/plan` when it is used as the leading composer command. */
export function consumePlanCommand(text: string): {
  text: string;
  planning: boolean;
} {
  const match = text.match(/^\s*\/plan(?=\s|$)\s*/i);
  if (!match) return { text, planning: false };
  return { text: text.slice(match[0].length), planning: true };
}

export function planTurnPrompt(request: string): string {
  return [
    "You are in plan mode. Investigate the request and the repository, but do not modify files, run destructive commands, or start implementing.",
    "Resolve important implementation details and finish with one self-contained Markdown plan. The plan must be specific enough to build after explicit user approval.",
    "Structure the final plan with a Markdown heading and concrete implementation steps.",
    "Do not ask the user to approve inside the response; the application provides a separate Build action.",
    "",
    "## Request",
    "",
    request.trim(),
  ].join("\n");
}

export function buildPlanPrompt(plan: string): string {
  return [
    "The user reviewed and explicitly approved the following implementation plan. Implement it now, using this exact edited version as the source of truth.",
    "",
    "<approved_plan>",
    plan.trim(),
    "</approved_plan>",
  ].join("\n");
}

/** Provider messages that can arrive as ordinary assistant text despite no work occurring. */
export function isProviderFailureText(text: string): boolean {
  const value = text.trim();
  if (!value) return false;
  return [
    /(?:^|\n)\s*upgrade your plan to continue[.!]?\s*(?:$|\n)/i,
    /(?:^|\n)\s*(?:you(?:'ve| have) )?reached (?:your )?(?:usage|request|spend) limit/i,
    /(?:^|\n)\s*(?:usage|rate|request) limit (?:reached|exceeded)/i,
    /(?:^|\n)\s*(?:authentication required|please (?:sign|log) in to continue)/i,
  ].some((pattern) => pattern.test(value));
}

/** A fallback assistant response must look like an authored Markdown plan. */
export function isReviewablePlan(text: string): boolean {
  const value = text.trim();
  if (!value || isProviderFailureText(value)) return false;
  const hasHeading = /^\s{0,3}#{1,6}\s+\S/m.test(value);
  const steps = value.match(/^\s*(?:[-*+] |\d+[.)] )\S/gm)?.length ?? 0;
  return hasHeading || steps >= 2;
}

/** First markdown heading, or the first non-empty prose line. */
export function planTitle(text: string): string {
  const heading = text.match(/^\s{0,3}#{1,6}\s+(.+)$/m);
  if (heading) return unwrapMarkdown(heading[1]);
  for (const line of text.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("```") || trimmed === "---") continue;
    return unwrapMarkdown(trimmed).slice(0, 80) || "Plan";
  }
  return "Plan";
}

/** First prose paragraph that is not the title heading. */
export function planSummary(text: string): string {
  const title = planTitle(text);
  const parts: string[] = [];
  let inFence = false;
  for (const line of text.split(/\r?\n/)) {
    if (/^\s*```/.test(line)) {
      inFence = !inFence;
      continue;
    }
    if (inFence || /^\s{0,3}#{1,6}\s+/.test(line)) continue;
    const trimmed = line.trim();
    if (!trimmed || trimmed === "---") continue;
    const next = unwrapMarkdown(trimmed.replace(/^[-*+]\s+/, ""));
    if (!next || next === title) continue;
    parts.push(next);
    if (parts.join(" ").length >= 140) break;
  }
  const summary = parts.join(" ");
  if (!summary) return "";
  return summary.length > 140 ? `${summary.slice(0, 139)}…` : summary;
}

export function planMeta(text: string): string[] {
  const parts: string[] = ["Plan"];
  const headings = text.match(/^\s{0,3}#{1,6}\s+/gm)?.length ?? 0;
  if (headings > 1) parts.push(`${headings} sections`);
  if (/```\s*mermaid\b/i.test(text)) parts.push("diagram");
  const words = text.trim().split(/\s+/).filter(Boolean).length;
  if (words > 0) parts.push(`${words} words`);
  return parts;
}

function unwrapMarkdown(value: string): string {
  return value
    .replace(/!\[[^\]]*]\([^)]*\)/g, "")
    .replace(/\[([^\]]+)]\([^)]*\)/g, "$1")
    .replace(/[*_`]+/g, "")
    .trim();
}
