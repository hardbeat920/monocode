export type OpenClawGatewayFrame =
  | OpenClawConnectChallenge
  | OpenClawHelloOk
  | OpenClawRequest
  | OpenClawResponse
  | OpenClawEvent;

export type OpenClawConnectChallenge = {
  type: "event";
  event: "connect.challenge";
  payload: Record<string, unknown>;
};

export type OpenClawHelloOk = {
  type: "hello-ok";
  snapshot?: Record<string, unknown>;
  limits?: Record<string, unknown>;
};

export type OpenClawRequest = {
  type: "req";
  id: string;
  method: string;
  params?: unknown;
};

export type OpenClawResponse = {
  type: "res";
  id: string;
  ok: boolean;
  payload?: unknown;
  error?: { code?: string; message?: string; details?: unknown };
};

export type OpenClawEvent = {
  type: "event";
  event: string;
  payload?: unknown;
  seq?: number;
};

export function parseOpenClawFrame(line: string): OpenClawGatewayFrame {
  let value: unknown;
  try {
    value = JSON.parse(line);
  } catch {
    throw new Error("OpenClaw Gateway sent invalid JSON");
  }
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("OpenClaw Gateway sent an invalid frame");
  }
  const frame = value as Record<string, unknown>;
  if (frame.type === "hello-ok") return frame as unknown as OpenClawHelloOk;
  if (frame.type === "req" && typeof frame.id === "string" && typeof frame.method === "string") {
    return frame as unknown as OpenClawRequest;
  }
  if (frame.type === "res" && typeof frame.id === "string" && typeof frame.ok === "boolean") {
    return frame as unknown as OpenClawResponse;
  }
  if (frame.type === "event" && typeof frame.event === "string") {
    return frame as unknown as OpenClawEvent;
  }
  throw new Error("OpenClaw Gateway sent an unsupported frame");
}

export function openClawRequest(id: string, method: string, params?: unknown): OpenClawRequest {
  if (!id || !method) throw new Error("OpenClaw Gateway request requires id and method");
  return { type: "req", id, method, ...(params === undefined ? {} : { params }) };
}

/** The first Gateway frame is a connect request; auth fields remain native-only. */
export function openClawConnectRequest(id: string, client: Record<string, unknown>): OpenClawRequest {
  return openClawRequest(id, "connect", { client });
}
