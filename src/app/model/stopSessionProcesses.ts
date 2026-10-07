type Failure = { error: unknown };

const failure = (step: () => Promise<unknown>): Promise<Failure | undefined> =>
  step().then(
    () => undefined,
    (error: unknown) => ({ error }),
  );

/**
 * Stops a session's harness children, then always reaps its process and
 * finishes its control turn. Every step runs even when an earlier one fails,
 * and the first failure in that order is rethrown afterwards: a child that
 * would not stop, then a kill that failed, then the control turn. A process
 * that outlived the kill may still be writing files, so the caller must not
 * treat the worker as stopped.
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
  const failures = [
    await failure(stopChildren),
    await failure(kill),
    await failure(finishTurn),
  ];
  const first = failures.find((entry) => entry !== undefined);
  if (first) throw first.error;
}
