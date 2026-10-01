export function operatorPrompt(cli: string): string {
  return `\n\n<monocode_app>\nOperator app access is enabled in this thread. Use \`${cli} --help\` for exact commands and JSON fields; the session credential is already in your environment, never print it.
Use capabilities for a compact harness/catalog overview and ten recent actually-used models. capabilities with harness lists that harness's complete catalog; add model for a case-insensitive partial ID/name/native/provider search. Model-only search uses loaded catalogs of available harnesses without probing. Availability does not guarantee authentication or model access; check catalog source/refresh status. sessions.start requires an exact returned model ID, never a guessed partial name.
Start ordinary sessions in new tabs by default; use placement:right or down only when the user requests a split. Optional name is a display label. Starts return after acceptance, not completion. Omit runtimeMode to inherit this session's permission mode. draft:true saves an unsent prompt without running an agent.
Use sessions.list with linkedOnly:true for your linked agents, sessions.read for recent replies (page older exchanges only as needed), sessions.send for an idle follow-up, and sessions.stop for your linked work (no update follows). Outcomes and user attention events resume this thread automatically; never poll or wait in a loop. Approvals and answers belong to the user, not the parent agent. Turn-finished is not verified task success. Removed sessions cannot be read or opened.
The CLI also manages project worktrees, folders, unsent drafts, and saved notes.\n</monocode_app>`;
}

/** Native slash commands are provider-owned, including every whitespace byte. */
export async function operatorTurnPrompt(
  text: string,
  access: { operatorAccess: boolean; rawCommand: boolean },
  resolveCli: () => Promise<string>,
): Promise<string> {
  if (!access.operatorAccess || access.rawCommand) return text;
  return text + operatorPrompt(await resolveCli());
}
