import type { Session } from "../model/session";

/** Closed chats stay here so clicking a session card can paint without disk. */
export const SESSION_LOAD_CACHE_LIMIT = 12;
export const SESSION_LOAD_CACHE_MAX_BYTES = 32 * 1024 * 1024;

const estimatedBytes = new WeakMap<object, number>();

/** Conservative retained-size estimate without allocating a serialized copy. */
export function estimateSessionCacheBytes(session: Session): number {
  const known = estimatedBytes.get(session);
  if (known != null) return known;

  let bytes = 0;
  const seen = new WeakSet<object>();
  const pending: unknown[] = [session];
  while (pending.length > 0) {
    const value = pending.pop();
    if (typeof value === "string") {
      bytes += value.length * 2;
      continue;
    }
    if (typeof value === "number") {
      bytes += 8;
      continue;
    }
    if (typeof value === "boolean") {
      bytes += 4;
      continue;
    }
    if (!value || typeof value !== "object" || seen.has(value)) continue;
    seen.add(value);

    if (Array.isArray(value)) {
      bytes += 24 + value.length * 8;
      for (const item of value) pending.push(item);
      continue;
    }

    const values = Object.values(value);
    bytes += 32 + values.length * 8;
    for (const item of values) pending.push(item);
  }

  estimatedBytes.set(session, bytes);
  return bytes;
}

export function rememberLoadedSession(
  cache: Map<string, Session>,
  session: Session,
  limit = SESSION_LOAD_CACHE_LIMIT,
  maxBytes = SESSION_LOAD_CACHE_MAX_BYTES,
) {
  cache.delete(session.id);
  const sessionBytes = estimateSessionCacheBytes(session);
  if (limit <= 0 || sessionBytes > maxBytes) return;

  cache.set(session.id, session);
  let cacheBytes = 0;
  for (const cached of cache.values()) {
    cacheBytes += estimateSessionCacheBytes(cached);
  }
  while (cache.size > limit || cacheBytes > maxBytes) {
    const oldest = cache.keys().next().value;
    if (oldest == null) break;
    const evicted = cache.get(oldest);
    if (evicted) cacheBytes -= estimateSessionCacheBytes(evicted);
    cache.delete(oldest);
  }
}
