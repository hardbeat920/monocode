import type { AgentModel } from "./models";

export function isAstraModel(model: AgentModel): boolean {
  return [model.id, model.nativeId, model.name].some(
    (value) => value != null && /(^|[^a-z0-9])astra([^a-z0-9]|$)/i.test(value),
  );
}
