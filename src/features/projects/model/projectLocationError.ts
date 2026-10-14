import { t } from "../../../shared/i18n/locale";
import { displayPath } from "../../../shared/lib/paths";

/** Requires reconnecting the project before the user retries submission. */
export class ProjectNotFoundError extends Error {
  constructor(cwd: string) {
    super(t("projects.error.notFound", { path: displayPath(cwd) }));
    this.name = "ProjectNotFoundError";
  }
}
