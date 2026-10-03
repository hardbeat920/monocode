import {
  HARNESSES,
  type HandoffMeta,
  type HarnessId,
  type ModelTarget,
  type Session,
} from "./session";
import { sameProviderAccountId } from "../../providers/model/providerAccounts";

export type ProviderBinding = {
  harness: HarnessId;
  providerSessionId: string;
  providerAccountId?: string;
  cwd: string;
  deliveredThroughBlockId?: string;
  contextUsed?: number;
  contextWindow?: number;
};

export type ProviderContextDelivery = {
  switchId: string;
  status: "preparing" | "imported" | "accepted" | "uncertain";
  mode: "pending" | "native" | "inline";
  from: HarnessId;
  to: HarnessId;
  cwd: string;
  providerAccountId?: string;
  currentUserBlockId: string;
  sourceThroughBlockId?: string;
  includedBlockIds: string[];
  omittedBlockIds: string[];
  targetProviderSessionId?: string;
};

export type ProviderContextState = {
  version: 1;
  bindings: ProviderBinding[];
  delivery?: ProviderContextDelivery;
};

/** The running turn keeps its provider and model while the picker changes. */
export function runningProviderSelection(
  session: Session,
  recorded?: ModelTarget,
): ModelTarget {
  if (session.busy && recorded) return recorded;
  if (session.busy && session.pendingSwitch)
    return {
      harness: session.pendingSwitch.from,
      model: session.pendingSwitch.fromModel,
      modelSettings: session.pendingSwitch.fromSettings,
    };
  return {
    harness: session.harness,
    model: session.model,
    modelSettings: session.modelSettings,
  };
}

export function canApplyRunningConfiguration(
  session: Session,
  running: ModelTarget,
  revisions?: { running: number; selected: number },
): boolean {
  if (revisions)
    return (
      session.harness === running.harness &&
      revisions.running === revisions.selected
    );
  if (session.harness !== running.harness || session.model !== running.model)
    return false;
  const keys = new Set([
    ...Object.keys(session.modelSettings),
    ...Object.keys(running.modelSettings),
  ]);
  return [...keys].every(
    (key) => session.modelSettings[key] === running.modelSettings[key],
  );
}

const harnessIds = new Set<string>(HARNESSES);
const text = (value: unknown): string | undefined =>
  typeof value === "string" && value.trim() && !value.includes("\0")
    ? value
    : undefined;
const finiteUsage = (value: unknown): value is number =>
  typeof value === "number" && Number.isFinite(value) && value >= 0;

export function sanitizeProviderContext(
  value: unknown,
): ProviderContextState | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value))
    return undefined;
  const state = value as Partial<ProviderContextState>;
  if (state.version !== 1 || !Array.isArray(state.bindings)) return undefined;
  const bindings: ProviderBinding[] = [];
  for (const entry of state.bindings) {
    if (!entry || typeof entry !== "object" || !harnessIds.has(entry.harness))
      continue;
    const cwd = text(entry.cwd);
    const providerSessionId = text(entry.providerSessionId);
    if (!cwd || !providerSessionId) continue;
    const binding: ProviderBinding = {
      harness: entry.harness,
      cwd,
      providerSessionId,
      ...(text(entry.providerAccountId)
        ? { providerAccountId: entry.providerAccountId }
        : {}),
      ...(text(entry.deliveredThroughBlockId)
        ? { deliveredThroughBlockId: entry.deliveredThroughBlockId }
        : {}),
      ...(finiteUsage(entry.contextUsed)
        ? { contextUsed: entry.contextUsed }
        : {}),
      ...(finiteUsage(entry.contextWindow) && entry.contextWindow > 0
        ? { contextWindow: entry.contextWindow }
        : {}),
    };
    const index = bindings.findIndex((saved) =>
      sameSelection(
        saved,
        binding.harness,
        binding.cwd,
        binding.providerAccountId,
      ),
    );
    if (index < 0) bindings.push(binding);
    else bindings[index] = binding;
  }
  const result: ProviderContextState = { version: 1, bindings };
  const delivery = state.delivery;
  if (
    delivery &&
    typeof delivery === "object" &&
    text(delivery.switchId) &&
    text(delivery.cwd) &&
    text(delivery.currentUserBlockId) &&
    harnessIds.has(delivery.from) &&
    harnessIds.has(delivery.to) &&
    ["preparing", "imported", "accepted", "uncertain"].includes(
      delivery.status,
    ) &&
    ["pending", "native", "inline"].includes(delivery.mode) &&
    Array.isArray(delivery.includedBlockIds) &&
    Array.isArray(delivery.omittedBlockIds)
  ) {
    result.delivery = {
      switchId: delivery.switchId,
      status: delivery.status,
      mode: delivery.mode,
      from: delivery.from,
      to: delivery.to,
      cwd: delivery.cwd,
      currentUserBlockId: delivery.currentUserBlockId,
      includedBlockIds: [
        ...new Set(delivery.includedBlockIds.filter((id) => text(id))),
      ],
      omittedBlockIds: [
        ...new Set(delivery.omittedBlockIds.filter((id) => text(id))),
      ],
      ...(text(delivery.providerAccountId)
        ? { providerAccountId: delivery.providerAccountId }
        : {}),
      ...(text(delivery.sourceThroughBlockId)
        ? { sourceThroughBlockId: delivery.sourceThroughBlockId }
        : {}),
      ...(text(delivery.targetProviderSessionId)
        ? { targetProviderSessionId: delivery.targetProviderSessionId }
        : {}),
    };
  }
  return result;
}

function sameSelection(
  binding: Pick<ProviderBinding, "harness" | "cwd" | "providerAccountId">,
  harness: HarnessId,
  cwd: string,
  providerAccountId?: string,
): boolean {
  return (
    binding.harness === harness &&
    binding.cwd === cwd &&
    sameProviderAccountId(
      binding.providerAccountId || undefined,
      providerAccountId || undefined,
    )
  );
}

export function providerBinding(
  session: Session,
  harness: HarnessId,
  cwd: string,
  providerAccountId?: string,
): ProviderBinding | undefined {
  const saved = session.providerContext?.bindings.find((entry) =>
    sameSelection(entry, harness, cwd, providerAccountId),
  );
  if (saved) return saved;
  if (
    session.harness === harness &&
    session.providerSessionId &&
    sameProviderAccountId(
      session.providerAccountId || undefined,
      providerAccountId || undefined,
    ) &&
    (session.worktreeCwd ?? session.cwd) === cwd
  ) {
    return {
      harness,
      cwd,
      providerSessionId: session.providerSessionId,
      providerAccountId,
      deliveredThroughBlockId: session.blocks[session.blocks.length - 1]?.id,
      ...(session.context
        ? {
            contextUsed: session.context.used,
            contextWindow: session.context.window,
          }
        : {}),
    };
  }
  const source = session.pendingSwitch;
  if (
    source?.from === harness &&
    source.fromProviderSessionId &&
    sameProviderAccountId(
      source.fromProviderAccountId || undefined,
      providerAccountId || undefined,
    ) &&
    (session.worktreeCwd ?? session.cwd) === cwd
  ) {
    return {
      harness,
      cwd,
      providerSessionId: source.fromProviderSessionId,
      providerAccountId,
      deliveredThroughBlockId: session.blocks[session.blocks.length - 1]?.id,
    };
  }
  return undefined;
}

export function canResumeProviderBinding(
  session: Session,
  binding: ProviderBinding | undefined,
): binding is ProviderBinding {
  return (
    !!binding?.deliveredThroughBlockId &&
    session.blocks.some((block) => block.id === binding.deliveredThroughBlockId)
  );
}

export function rememberProviderBinding(
  session: Session,
  binding: ProviderBinding,
): Session {
  const state: ProviderContextState = session.providerContext ?? {
    version: 1,
    bindings: [],
  };
  const bindings = state.bindings.filter(
    (entry) =>
      !sameSelection(
        entry,
        binding.harness,
        binding.cwd,
        binding.providerAccountId,
      ),
  );
  bindings.push(binding);
  return { ...session, providerContext: { ...state, bindings } };
}

/** A startup identity is separate from proof that the target accepted a turn. */
export function recordProviderBound(
  session: Session,
  harness: HarnessId,
  cwd: string,
  providerSessionId: string,
  providerAccountId?: string,
): Session {
  const saved = providerBinding(session, harness, cwd, providerAccountId);
  let next = rememberProviderBinding(session, {
    ...(saved?.providerSessionId === providerSessionId ? saved : {}),
    harness,
    cwd,
    providerAccountId,
    providerSessionId,
    ...(saved?.providerSessionId === providerSessionId &&
    saved.deliveredThroughBlockId
      ? { deliveredThroughBlockId: saved.deliveredThroughBlockId }
      : {}),
  });
  if (next.pendingSwitch?.from === harness && next.harness !== harness) {
    next = {
      ...next,
      pendingSwitch: {
        ...next.pendingSwitch,
        fromProviderSessionId: providerSessionId,
      },
    };
  } else if (next.harness === harness) {
    next = { ...next, providerSessionId, providerAccountId };
  }
  const delivery = next.providerContext?.delivery;
  if (
    delivery &&
    delivery.status !== "accepted" &&
    sameSelection(
      {
        harness: delivery.to,
        cwd: delivery.cwd,
        providerAccountId: delivery.providerAccountId,
      },
      harness,
      cwd,
      providerAccountId,
    )
  ) {
    next = {
      ...next,
      providerContext: {
        ...next.providerContext!,
        delivery: { ...delivery, targetProviderSessionId: providerSessionId },
      },
    };
  }
  return next;
}

export function beginProviderDelivery(
  session: Session,
  delivery: Omit<ProviderContextDelivery, "status" | "mode">,
): Session {
  const blocks = session.blocks.slice();
  for (let index = blocks.length - 1; index >= 0; index--) {
    const handoff = blocks[index].handoff;
    if (!handoff) continue;
    blocks[index] = {
      ...blocks[index],
      handoff: {
        ...handoff,
        transfer: {
          switchId: delivery.switchId,
          status: "preparing",
          mode: "pending",
          included: delivery.includedBlockIds.length,
          omitted: delivery.omittedBlockIds.length,
          historicalAttachments: 0,
        },
      },
    };
    break;
  }
  return {
    ...session,
    blocks,
    providerContext: {
      ...(session.providerContext ?? { version: 1, bindings: [] }),
      delivery: { ...delivery, status: "preparing", mode: "pending" },
    },
  };
}

export function updateProviderHandoff(
  session: Session,
  switchId: string,
  update: Partial<NonNullable<HandoffMeta["transfer"]>>,
): Session {
  return {
    ...session,
    blocks: session.blocks.map((block) =>
      block.handoff?.transfer?.switchId === switchId
        ? {
            ...block,
            handoff: {
              ...block.handoff,
              transfer: { ...block.handoff.transfer, ...update },
            },
          }
        : block,
    ),
  };
}

export function markProviderContextDelivered(
  session: Session,
  switchId: string,
  mode: "native" | "inline",
  providerSessionId?: string,
  coverage?: {
    includedBlockIds?: string[];
    omittedBlockIds?: string[];
    sourceThroughBlockId?: string;
  },
): Session {
  const state = session.providerContext;
  const delivery = state?.delivery;
  if (
    !delivery ||
    delivery.switchId !== switchId ||
    delivery.status === "accepted" ||
    delivery.status === "uncertain"
  )
    return session;
  return updateProviderHandoff(
    {
      ...session,
      providerContext: {
        ...state!,
        delivery: {
          ...delivery,
          ...coverage,
          status: "imported",
          mode,
          ...(providerSessionId
            ? { targetProviderSessionId: providerSessionId }
            : {}),
        },
      },
    },
    switchId,
    {
      status: "imported",
      mode,
      ...(coverage?.includedBlockIds
        ? { included: coverage.includedBlockIds.length }
        : {}),
      ...(coverage?.omittedBlockIds
        ? { omitted: coverage.omittedBlockIds.length }
        : {}),
    },
  );
}

export function acceptProviderDelivery(
  session: Session,
  switchId: string,
): Session {
  const state = session.providerContext;
  const delivery = state?.delivery;
  if (
    !delivery ||
    delivery.switchId !== switchId ||
    delivery.status === "accepted" ||
    delivery.status === "uncertain"
  )
    return session;
  return updateProviderHandoff(
    {
      ...session,
      pendingSwitch: undefined,
      providerContext: {
        ...state!,
        delivery: { ...delivery, status: "accepted" },
      },
    },
    switchId,
    { status: "accepted" },
  );
}

export function failProviderDelivery(
  session: Session,
  switchId: string,
): Session {
  const state = session.providerContext;
  const delivery = state?.delivery;
  if (
    !delivery ||
    delivery.switchId !== switchId ||
    delivery.status === "accepted"
  )
    return session;
  return updateProviderHandoff(
    {
      ...session,
      blocks: session.blocks.map((block) =>
        block.id === delivery.currentUserBlockId && block.role === "user"
          ? { ...block, draft: true }
          : block,
      ),
      providerSessionId:
        session.harness === delivery.to ? undefined : session.providerSessionId,
      providerContext: {
        ...state!,
        bindings: state!.bindings.filter(
          (entry) =>
            !sameSelection(
              entry,
              delivery.to,
              delivery.cwd,
              delivery.providerAccountId,
            ),
        ),
        delivery: { ...delivery, status: "uncertain" },
      },
    },
    switchId,
    { status: "uncertain" },
  );
}

/** Snapshot preparation may stop before a delivery receipt exists. */
export function failUnstartedProviderRequest(
  session: Session,
  preparingHandoffId?: string,
): Session {
  if (!session.pendingSwitch) return session;
  let divider = -1;
  for (let index = session.blocks.length - 1; index >= 0; index--) {
    if (
      session.blocks[index].handoff?.status === "preparing" ||
      session.blocks[index].id === preparingHandoffId
    ) {
      divider = index;
      break;
    }
  }
  if (divider < 0) return session;
  const user = session.blocks
    .slice(divider + 1)
    .find((block) => block.role === "user");
  if (!user) return session;
  return {
    ...session,
    blocks: session.blocks.map((block) =>
      block.id === user.id ? { ...block, draft: true } : block,
    ),
  };
}

export function requiresFreshProviderBinding(
  session: Session,
  harness: HarnessId,
  cwd: string,
  providerAccountId?: string,
): boolean {
  const delivery = session.providerContext?.delivery;
  return Boolean(
    delivery &&
    delivery.status !== "accepted" &&
    sameSelection(
      {
        harness: delivery.to,
        cwd: delivery.cwd,
        providerAccountId: delivery.providerAccountId,
      },
      harness,
      cwd,
      providerAccountId,
    ),
  );
}

export function settleProviderBinding(
  session: Session,
  harness: HarnessId,
  cwd: string,
  providerAccountId?: string,
): Session {
  const binding = providerBinding(session, harness, cwd, providerAccountId);
  if (!binding) return session;
  return rememberProviderBinding(session, {
    ...binding,
    deliveredThroughBlockId: session.blocks[session.blocks.length - 1]?.id,
    ...(session.harness === harness && session.context
      ? {
          contextUsed: session.context.used,
          contextWindow: session.context.window,
        }
      : {}),
  });
}

export function recordProviderContextUsage(
  session: Session,
  harness: HarnessId,
  cwd: string,
  usage: { used?: number; window?: number },
  providerAccountId?: string,
): Session {
  const binding = providerBinding(session, harness, cwd, providerAccountId);
  if (!binding) return session;
  return rememberProviderBinding(session, {
    ...binding,
    ...(finiteUsage(usage.used) ? { contextUsed: usage.used } : {}),
    ...(finiteUsage(usage.window) && usage.window > 0
      ? { contextWindow: usage.window }
      : {}),
  });
}
