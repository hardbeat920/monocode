import {
  closeHarnessSse,
  harnessHttp,
  openHarnessSse,
  watchSse,
} from "../../core/child";
import {
  asRecord,
  normalizeOpenCodeV2Event,
  stringField,
  toOpenCodeV2PermissionRules,
  type OpenCodeApiGeneration,
  type OpenCodePermissionRule,
} from "./opencodeProtocol";
import {
  OpenCodeV2EventTranslator,
  openCodeV2ContentText,
} from "./opencodeV2Events";

export class OpenCodeHttpError extends Error {
  readonly status: number;
  readonly body: unknown;

  constructor(status: number, body: unknown, message: string) {
    super(message);
    this.name = status === 404 ? "NotFoundError" : "OpenCodeHttpError";
    this.status = status;
    this.body = body;
  }
}

export type OpenCodeSession = {
  id: string;
  parentID?: string;
  directory?: string;
  title?: string;
};

export type OpenCodePromptPart =
  | { type: "text"; text: string }
  | { type: "file"; mime: string; filename: string; url: string };

export type OpenCodeMessage = {
  info?: Record<string, unknown>;
  parts?: unknown[];
};

export class OpenCodeClient {
  private readonly permissionSessions = new Map<string, string>();
  private readonly configuredModels = new Map<string, string>();
  private readonly configuredAgents = new Map<string, string>();
  private readonly forms = new Map<
    string,
    {
      sessionID: string;
      fields: Array<{
        key: string;
        type?: string;
        optionValues: Map<string, string>;
      }>;
    }
  >();

  constructor(
    readonly baseUrl: string,
    readonly directory: string,
    readonly generation: OpenCodeApiGeneration = "v1",
    readonly serverPassword?: string,
  ) {}

  async getSession(sessionID: string): Promise<OpenCodeSession> {
    return this.request<OpenCodeSession>(
      "GET",
      this.path(`/session/${enc(sessionID)}`),
    );
  }

  async getMessages(sessionID: string): Promise<OpenCodeMessage[]> {
    const messages = await this.request<unknown[]>(
      "GET",
      this.path(`/session/${enc(sessionID)}/message`),
    );
    if (this.generation === "v1") return messages as OpenCodeMessage[];
    return messages.map(normalizeV2Message);
  }

  async createSession(input: {
    title?: string;
    permission?: unknown;
  }): Promise<OpenCodeSession> {
    const permission = input.permission as OpenCodePermissionRule[] | undefined;
    return this.request<OpenCodeSession>("POST", this.path("/session"), {
      body: {
        ...(input.title ? { title: input.title } : {}),
        ...(this.generation === "v2"
          ? {
              location: { directory: this.directory },
              ...(permission
                ? { permissions: toOpenCodeV2PermissionRules(permission) }
                : {}),
            }
          : input.permission
            ? { permission: input.permission }
            : {}),
      },
    });
  }

  async updateSession(
    sessionID: string,
    body: Record<string, unknown>,
  ): Promise<OpenCodeSession> {
    const normalizedBody =
      this.generation === "v2" && Array.isArray(body.permission)
        ? {
            ...body,
            permission: undefined,
            permissions: toOpenCodeV2PermissionRules(
              body.permission as OpenCodePermissionRule[],
            ),
          }
        : body;
    return this.request<OpenCodeSession>(
      "PATCH",
      this.path(`/session/${enc(sessionID)}`),
      {
        body: normalizedBody,
      },
    );
  }

  async forkSession(
    sessionID: string,
    directory: string,
  ): Promise<OpenCodeSession> {
    const forked = await this.request<OpenCodeSession>(
      "POST",
      this.path(`/session/${enc(sessionID)}/fork`),
      this.generation === "v2"
        ? { body: {} }
        : { query: { directory }, body: {} },
    );
    if (this.generation === "v2") {
      await this.request<unknown>(
        "POST",
        this.path(`/session/${enc(forked.id)}/move`),
        { body: { directory, delivery: "queue" } },
      );
    }
    return forked;
  }

  async abortSession(sessionID: string): Promise<void> {
    await this.request<unknown>(
      "POST",
      this.path(
        `/session/${enc(sessionID)}/${this.generation === "v2" ? "interrupt" : "abort"}`,
      ),
      this.generation === "v2" ? { query: { resume: "false" } } : { body: {} },
    ).catch(() => undefined);
  }

  async revertSession(sessionID: string, messageID: string): Promise<void> {
    if (this.generation === "v2") {
      await this.request<unknown>(
        "POST",
        this.path(`/session/${enc(sessionID)}/revert/stage`),
        { body: { messageID } },
      );
      await this.request<unknown>(
        "POST",
        this.path(`/session/${enc(sessionID)}/revert/commit`),
      );
      return;
    }
    await this.request<unknown>("POST", `/session/${enc(sessionID)}/revert`, {
      body: { messageID },
    });
  }

  async summarizeSession(
    sessionID: string,
    model: { providerID: string; modelID: string },
  ): Promise<void> {
    if (this.generation === "v2") {
      await this.setModel(sessionID, model);
      await this.request<unknown>(
        "POST",
        this.path(`/session/${enc(sessionID)}/compact`),
        {
          body: { delivery: "queue" },
          timeoutMs: 30 * 60_000,
        },
      );
      return;
    }
    await this.request<unknown>(
      "POST",
      `/session/${enc(sessionID)}/summarize`,
      {
        body: model,
        timeoutMs: 30 * 60_000,
      },
    );
  }

  async promptAsync(input: {
    sessionID: string;
    model: { providerID: string; modelID: string };
    agent?: string;
    variant?: string;
    parts: OpenCodePromptPart[];
    delivery?: "queue" | "steer";
  }): Promise<void> {
    if (this.generation === "v2") {
      await this.configureTurn(input);
      await this.request<unknown>(
        "POST",
        this.path(`/session/${enc(input.sessionID)}/prompt`),
        { body: v2PromptBody(input.parts, input.delivery ?? "queue") },
      );
      return;
    }
    await this.request<unknown>(
      "POST",
      `/session/${enc(input.sessionID)}/prompt_async`,
      {
        body: {
          model: input.model,
          ...(input.agent ? { agent: input.agent } : {}),
          ...(input.variant ? { variant: input.variant } : {}),
          parts: input.parts,
        },
      },
    );
  }

  async prompt(input: {
    sessionID: string;
    model: { providerID: string; modelID: string };
    agent?: string;
    variant?: string;
    parts: OpenCodePromptPart[];
    timeoutMs?: number;
  }): Promise<{ info?: Record<string, unknown>; parts?: unknown[] }> {
    if (this.generation === "v2") {
      await this.configureTurn(input);
      const result = await this.request<{ text?: string }>(
        "POST",
        this.path(`/session/${enc(input.sessionID)}/generate`),
        {
          body: { prompt: promptText(input.parts) },
          timeoutMs: input.timeoutMs,
        },
      );
      return {
        info: {},
        parts: result.text ? [{ type: "text", text: result.text }] : [],
      };
    }
    return this.request("POST", `/session/${enc(input.sessionID)}/message`, {
      body: {
        model: input.model,
        ...(input.agent ? { agent: input.agent } : {}),
        ...(input.variant ? { variant: input.variant } : {}),
        parts: input.parts,
      },
      timeoutMs: input.timeoutMs,
    });
  }

  async replyPermission(
    requestID: string,
    reply: "once" | "always" | "reject",
  ): Promise<void> {
    if (this.generation === "v2") {
      const sessionID = this.permissionSessions.get(requestID);
      if (!sessionID) {
        throw new Error(
          `OpenCode permission ${requestID} has no owning session`,
        );
      }
      await this.request<unknown>(
        "POST",
        this.path(
          `/session/${enc(sessionID)}/permission/${enc(requestID)}/reply`,
        ),
        { body: { decision: reply } },
      );
      this.permissionSessions.delete(requestID);
      return;
    }
    await this.request<unknown>("POST", `/permission/${enc(requestID)}/reply`, {
      body: { reply },
    });
  }

  async replyQuestion(requestID: string, answers: string[][]): Promise<void> {
    if (this.generation === "v2") {
      const form = this.forms.get(requestID);
      if (!form) throw new Error(`OpenCode form ${requestID} is not pending`);
      const answer = Object.fromEntries(
        form.fields.map((field, index) => {
          const values = answers[index] ?? [];
          const mapped = values.map(
            (value) => field.optionValues.get(value) ?? value,
          );
          const value =
            field.type === "boolean"
              ? /^yes|true$/i.test(mapped[0] ?? "")
              : field.type === "number" || field.type === "integer"
                ? Number(mapped[0] ?? "")
                : mapped.length <= 1
                  ? (mapped[0] ?? "")
                  : mapped;
          return [field.key, value];
        }),
      );
      await this.request<unknown>(
        "POST",
        this.path(
          `/session/${enc(form.sessionID)}/form/${enc(requestID)}/reply`,
        ),
        { body: { answer } },
      );
      this.forms.delete(requestID);
      return;
    }
    await this.request<unknown>("POST", `/question/${enc(requestID)}/reply`, {
      body: { answers },
    });
  }

  async rejectQuestion(requestID: string): Promise<void> {
    if (this.generation === "v2") {
      const form = this.forms.get(requestID);
      if (!form) throw new Error(`OpenCode form ${requestID} is not pending`);
      await this.request<unknown>(
        "DELETE",
        this.path(`/session/${enc(form.sessionID)}/form/${enc(requestID)}`),
      );
      this.forms.delete(requestID);
      return;
    }
    await this.request<unknown>("POST", `/question/${enc(requestID)}/reject`, {
      body: {},
    });
  }

  async subscribeEvents(
    sessionId: string,
    onEvent: (event: Record<string, unknown>) => void,
    onEnd?: (error?: string) => void,
  ): Promise<void> {
    const url = this.url(this.path("/event"));
    const translator =
      this.generation === "v2" ? new OpenCodeV2EventTranslator() : null;
    watchSse(
      sessionId,
      (data) => {
        const parsed = parseJson(data);
        const rec = asRecord(parsed);
        if (!rec) return;
        if (!translator) {
          onEvent(rec);
          return;
        }
        const translated = translator.translate(normalizeOpenCodeV2Event(rec));
        const mapped = translated && this.mapV2InteractiveEvent(translated);
        if (mapped) onEvent(mapped);
      },
      onEnd,
    );
    await openHarnessSse(sessionId, url, this.headers());
  }

  async closeEvents(sessionId: string): Promise<void> {
    await closeHarnessSse(sessionId).catch(() => undefined);
    this.permissionSessions.clear();
    this.forms.clear();
    this.configuredModels.clear();
    this.configuredAgents.clear();
  }

  async listModels(): Promise<unknown[]> {
    return this.request<unknown[]>("GET", this.path("/model"));
  }

  async listProviders(): Promise<unknown[]> {
    return this.request<unknown[]>("GET", this.path("/provider"));
  }

  async listAgents(): Promise<unknown[]> {
    return this.request<unknown[]>("GET", this.path("/agent"));
  }

  private async configureTurn(input: {
    sessionID: string;
    model: { providerID: string; modelID: string };
    agent?: string;
    variant?: string;
  }): Promise<void> {
    const updates: Promise<void>[] = [
      this.setModel(input.sessionID, input.model, input.variant),
    ];
    if (input.agent) updates.push(this.setAgent(input.sessionID, input.agent));
    await Promise.all(updates);
  }

  private async setModel(
    sessionID: string,
    model: { providerID: string; modelID: string },
    variant?: string,
  ): Promise<void> {
    const key = `${model.providerID}/${model.modelID}/${variant ?? ""}`;
    if (this.configuredModels.get(sessionID) === key) return;
    await this.request<unknown>(
      "POST",
      this.path(`/session/${enc(sessionID)}/model`),
      {
        body: {
          model: {
            providerID: model.providerID,
            id: model.modelID,
            ...(variant ? { variant } : {}),
          },
        },
      },
    );
    this.configuredModels.set(sessionID, key);
  }

  private async setAgent(sessionID: string, agent: string): Promise<void> {
    if (this.configuredAgents.get(sessionID) === agent) return;
    await this.request<unknown>(
      "POST",
      this.path(`/session/${enc(sessionID)}/agent`),
      { body: { agent } },
    );
    this.configuredAgents.set(sessionID, agent);
  }

  private mapV2InteractiveEvent(
    event: Record<string, unknown>,
  ): Record<string, unknown> | null {
    const type = stringField(event, "type");
    const properties = asRecord(event.properties) ?? {};
    if (type === "message.updated") {
      const info = asRecord(properties.info) ?? properties;
      const messageType = stringField(info, "type");
      if (
        !stringField(info, "role") &&
        (messageType === "user" || messageType === "assistant")
      ) {
        const model = asRecord(info.model);
        return {
          ...event,
          properties: {
            ...properties,
            info: {
              ...info,
              role: messageType,
              providerID:
                stringField(info, "providerID") ??
                stringField(model, "providerID"),
              modelID:
                stringField(info, "modelID") ??
                stringField(model, "id") ??
                stringField(model, "modelID"),
            },
          },
        };
      }
      return event;
    }
    if (type === "permission.asked") {
      const request = asRecord(properties.request) ?? properties;
      const id = stringField(request, "id");
      const sessionID = stringField(request, "sessionID");
      if (id && sessionID) {
        rememberPending(this.permissionSessions, id, sessionID);
      }
      const source = asRecord(request.source);
      const existingTool = asRecord(request.tool);
      return {
        ...event,
        properties: {
          ...request,
          permission:
            stringField(request, "permission") ??
            stringField(request, "action") ??
            "tool",
          patterns: Array.isArray(request.patterns)
            ? request.patterns
            : Array.isArray(request.resources)
              ? request.resources
              : [],
          tool:
            existingTool ??
            (source
              ? {
                  ...source,
                  callID: stringField(source, "id"),
                }
              : undefined),
        },
      };
    }
    if (type !== "form.created") return event;
    const form = asRecord(properties.form) ?? properties;
    const id = stringField(form, "id");
    const sessionID = stringField(form, "sessionID");
    const fields = Array.isArray(form.fields)
      ? form.fields.map(asRecord).filter((field) => field !== null)
      : [];
    if (!id || !sessionID) return null;
    const formFields = fields.map((field, index) => {
      const key = stringField(field, "key") ?? `field_${index}`;
      const optionValues = new Map<string, string>();
      for (const option of Array.isArray(field.options) ? field.options : []) {
        const rec = asRecord(option);
        const label = stringField(rec, "label");
        const value = stringField(rec, "value");
        if (label && value) optionValues.set(label, value);
      }
      return { key, type: stringField(field, "type"), optionValues };
    });
    rememberPending(this.forms, id, { sessionID, fields: formFields });
    return {
      ...event,
      type: "question.asked",
      properties: {
        id,
        sessionID,
        questions: fields.map((field, index) => {
          const fieldType = stringField(field, "type");
          const options = Array.isArray(field.options)
            ? field.options
                .map((option) => {
                  const rec = asRecord(option);
                  const label =
                    stringField(rec, "label") ??
                    stringField(rec, "value") ??
                    (typeof option === "string" ? option : undefined);
                  return label ? { label } : null;
                })
                .filter((option) => option !== null)
            : fieldType === "boolean"
              ? [{ label: "Yes" }, { label: "No" }]
              : [];
          return {
            id: formFields[index].key,
            header: stringField(field, "title"),
            question:
              stringField(field, "prompt") ??
              stringField(field, "title") ??
              stringField(field, "description") ??
              formFields[index].key,
            multiple: fieldType === "multiselect",
            custom: options.length === 0,
            options,
          };
        }),
      },
    };
  }

  private async request<T>(
    method: string,
    path: string,
    opts?: {
      body?: unknown;
      query?: Record<string, string>;
      timeoutMs?: number;
    },
  ): Promise<T> {
    const url = this.url(path, opts?.query);
    const hasBody = opts?.body !== undefined;
    const response = await harnessHttp({
      url,
      method,
      headers: this.headers(hasBody),
      body: hasBody ? JSON.stringify(opts?.body ?? {}) : undefined,
      timeoutMs: opts?.timeoutMs,
    });
    if (response.status === 204 || response.body.trim() === "") {
      if (response.status >= 400) {
        throw new OpenCodeHttpError(
          response.status,
          response.body,
          httpErrorMessage(response.status, response.body),
        );
      }
      return undefined as T;
    }
    const parsed = parseJson(response.body);
    if (response.status >= 400) {
      throw new OpenCodeHttpError(
        response.status,
        parsed,
        httpErrorMessage(response.status, response.body, parsed),
      );
    }
    return unwrapData<T>(parsed);
  }

  private url(path: string, query?: Record<string, string>): string {
    const url = new URL(path, `${this.baseUrl.replace(/\/$/, "")}/`);
    if (this.directory) url.searchParams.set("directory", this.directory);
    if (query) {
      for (const [key, value] of Object.entries(query)) {
        url.searchParams.set(key, value);
      }
    }
    return url.toString();
  }

  private headers(json = false): Record<string, string> {
    return {
      ...(json ? { "Content-Type": "application/json" } : {}),
      ...(this.directory
        ? { "x-opencode-directory": encodeURIComponent(this.directory) }
        : {}),
      ...(this.serverPassword
        ? {
            Authorization: `Basic ${btoa(`opencode:${this.serverPassword}`)}`,
          }
        : {}),
    };
  }

  private path(path: string): string {
    return this.generation === "v2" ? `/api${path}` : path;
  }
}

function enc(value: string): string {
  return encodeURIComponent(value);
}

function parseJson(raw: string): unknown {
  try {
    return JSON.parse(raw) as unknown;
  } catch {
    return raw;
  }
}

function unwrapData<T>(value: unknown): T {
  const rec = asRecord(value);
  if (rec && "data" in rec && rec.data !== undefined) return rec.data as T;
  return value as T;
}

function promptText(parts: OpenCodePromptPart[]): string {
  return parts
    .filter(
      (part): part is Extract<OpenCodePromptPart, { type: "text" }> =>
        part.type === "text",
    )
    .map((part) => part.text)
    .join("\n\n");
}

function v2PromptBody(
  parts: OpenCodePromptPart[],
  delivery: "queue" | "steer",
): Record<string, unknown> {
  return {
    text: promptText(parts),
    files: parts
      .filter(
        (part): part is Extract<OpenCodePromptPart, { type: "file" }> =>
          part.type === "file",
      )
      .map((part) => ({ uri: part.url, name: part.filename })),
    delivery,
  };
}

function normalizeV2Message(value: unknown): OpenCodeMessage {
  const rec = asRecord(value);
  if (!rec) return {};
  if (asRecord(rec.info)) return rec as OpenCodeMessage;
  const type = stringField(rec, "type");
  const role =
    type === "user" ? "user" : type === "assistant" ? "assistant" : undefined;
  const files = Array.isArray(rec.files)
    ? rec.files.map((file) => {
        const item = asRecord(file);
        return {
          type: "file",
          mime: stringField(item, "mime"),
          filename: stringField(item, "name"),
          url: stringField(item, "uri"),
        };
      })
    : [];
  const content = Array.isArray(rec.content)
    ? rec.content.map((part, index) => {
        const item = asRecord(part) ?? {};
        const partType = stringField(item, "type");
        const id = stringField(item, "id") ?? `${stringField(rec, "id")}:${index}`;
        const state = asRecord(item.state);
        return {
          ...item,
          id,
          messageID: stringField(rec, "id"),
          ...(partType === "tool"
            ? {
                tool: stringField(item, "name"),
                callID: id,
                ...(state
                  ? {
                      state: {
                        ...state,
                        output: state.output ?? openCodeV2ContentText(state.content),
                      },
                    }
                  : {}),
              }
            : {}),
        };
      })
    : [];
  return {
    info: { ...rec, ...(role ? { role } : {}) },
    parts: [
      ...(typeof rec.text === "string"
        ? [{ type: "text", text: rec.text }]
        : []),
      ...content,
      ...files,
    ],
  };
}

function rememberPending<K, V>(map: Map<K, V>, key: K, value: V): void {
  map.set(key, value);
  if (map.size <= 256) return;
  const oldest = map.keys().next().value;
  if (oldest !== undefined) map.delete(oldest);
}

function httpErrorMessage(
  status: number,
  raw: string,
  parsed: unknown = parseJson(raw),
): string {
  const rec = asRecord(parsed);
  const nested = asRecord(rec?.error) ?? asRecord(rec?.data);
  const message =
    (typeof rec?.message === "string" && rec.message.trim()) ||
    (typeof nested?.message === "string" && nested.message.trim()) ||
    raw.trim();
  return message || `OpenCode HTTP ${status}`;
}
