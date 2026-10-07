/**
 * Stops a session's harness children, then always reaps its process and
 * finishes its control turn. A kill failure is rethrown only after that
 * cleanup: a process that outlived the kill may still be writing files, so the
 * caller must not treat the worker as stopped. An error from stopping the
 * children takes precedence over a kill failure.
 */
export async function stopSessionProcesses({
  stopChildren,
  kill,
  finishTurn,
}: {
  stopChildren: () => Promise<unknown>;
  kill: () => Promise<unknown>;
  finishTurn: () => Promise<unknown>;
}): Promise<void> {
  let killFailure: { error: unknown } | undefined;
  try {
    await stopChildren();
  } finally {
    killFailure = await kill().then(
      () => undefined,
      (error: unknown) => ({ error }),
    );
    await finishTurn();
  }
  if (killFailure) throw killFailure.error;
}
