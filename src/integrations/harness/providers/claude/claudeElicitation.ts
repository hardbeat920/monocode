import type {
  UserQuestion,
  UserQuestionReply,
} from "../../../../features/sessions/model/userQuestion";
import { selectedAnswerLabels } from "../../../../features/sessions/model/userQuestion";
import { asRecord, stringField } from "./claudeProtocol";

function actionQuestionId(request: Record<string, unknown>): string {
  const properties =
    asRecord(asRecord(request.requested_schema)?.properties) ?? {};
  let id = "__mcp_action";
  while (Object.prototype.hasOwnProperty.call(properties, id)) id += "_";
  return id;
}

export function elicitationQuestions(
  request: Record<string, unknown>,
): UserQuestion[] {
  const schema = asRecord(request.requested_schema);
  const properties = asRecord(schema?.properties);
  if (request.mode === "url" || !properties)
    throw new Error("This MCP elicitation form is not supported.");
  const questions = Object.entries(properties).map(
    ([id, raw]): UserQuestion => {
      const field = asRecord(raw);
      const type = stringField(field, "type") ?? "string";
      if (!["string", "boolean", "number", "integer"].includes(type))
        throw new Error(`Unsupported MCP field type for ${id}.`);
      const choices = Array.isArray(field?.enum)
        ? field.enum
        : type === "boolean"
          ? [true, false]
          : [];
      return {
        id,
        prompt: stringField(field, "title") ?? id,
        header: stringField(field, "description"),
        multiSelect: false,
        allowCustom: choices.length === 0,
        options: choices.map((value) => ({
          id: String(value),
          label: String(value),
        })),
      };
    },
  );
  questions.push({
    id: actionQuestionId(request),
    prompt: "Submit this form?",
    multiSelect: false,
    allowCustom: false,
    options: ["accept", "decline", "cancel"].map((id) => ({
      id,
      label: id[0].toUpperCase() + id.slice(1),
    })),
  });
  return questions;
}

export function elicitationResponse(
  request: Record<string, unknown>,
  questions: UserQuestion[],
  reply: UserQuestionReply,
): Record<string, unknown> {
  if (reply.kind === "skipped") return { action: "cancel" };
  const actionId = actionQuestionId(request);
  const action = reply.answers[actionId]?.[0];
  if (action !== "accept")
    return { action: action === "decline" ? "decline" : "cancel" };
  const schema = asRecord(request.requested_schema);
  const properties = asRecord(schema?.properties) ?? {};
  const required = Array.isArray(schema?.required) ? schema.required : [];
  const content: Record<string, unknown> = Object.create(null);
  for (const question of questions) {
    if (question.id === actionId) continue;
    const text = selectedAnswerLabels(question, reply)[0];
    if (text === undefined || text === "") {
      if (required.includes(question.id))
        throw new Error(`${question.prompt} is required.`);
      continue;
    }
    const field = asRecord(properties[question.id]) ?? {};
    const value =
      field.type === "boolean"
        ? text === "true"
        : field.type === "number" || field.type === "integer"
          ? Number(text)
          : text;
    if (
      typeof value === "number" &&
      (!Number.isFinite(value) ||
        (field.type === "integer" && !Number.isInteger(value)))
    )
      throw new Error(`${question.prompt} must be a valid ${field.type}.`);
    if (field.type === "boolean" && text !== "true" && text !== "false")
      throw new Error(`${question.prompt} must be true or false.`);
    if (Array.isArray(field.enum) && !field.enum.includes(value))
      throw new Error(`${question.prompt} must use a listed value.`);
    if (
      typeof value === "number" &&
      ((typeof field.minimum === "number" && value < field.minimum) ||
        (typeof field.maximum === "number" && value > field.maximum))
    )
      throw new Error(`${question.prompt} is outside the allowed range.`);
    if (
      typeof value === "string" &&
      ((typeof field.minLength === "number" &&
        value.length < field.minLength) ||
        (typeof field.maxLength === "number" &&
          value.length > field.maxLength) ||
        (typeof field.pattern === "string" &&
          !new RegExp(field.pattern).test(value)))
    )
      throw new Error(
        `${question.prompt} does not match the requested format.`,
      );
    content[question.id] = value;
  }
  return { action: "accept", content };
}
