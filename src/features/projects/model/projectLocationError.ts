import { displayPath } from "../../../shared/lib/paths";

/** Requires reconnecting the project before the user retries submission. */
export class ProjectNotFoundError extends Error {
  constructor(cwd: string) {
    super(
      `Project folder not found: ${displayPath(cwd)}. Right-click the project and choose "Edit folder path…" to reconnect it.`,
    );
    this.name = "ProjectNotFoundError";
  }
}
