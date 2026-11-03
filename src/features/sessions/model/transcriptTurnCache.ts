import type { Block } from "./session";
import {
  groupMonoChatTurns,
  groupMonoTurnItems,
  groupTurnItems,
  groupTurns,
  monoTurnRuns,
  type TurnItem,
} from "./transcriptActivity";

/** Reuse unchanged history while immutable updates replace the live blocks. */
export class TranscriptTurnCache {
  private blocks: Block[] | undefined;
  private managed = false;
  private inlineWork = false;
  private turns: Block[][] = [];
  private tailStart = 0;
  private items = new WeakMap<Block[], Map<string, TurnItem[]>>();

  group(blocks: Block[], managed = false, inlineWork = false): Block[][] {
    if (
      this.blocks === blocks &&
      this.managed === managed &&
      this.inlineWork === inlineWork
    )
      return this.turns;
    // Providers replace the live tail immutably. Check the prefix for edits,
    // but don't allocate and regroup every historical turn on every token.
    const reusePrefix =
      this.blocks !== undefined &&
      this.managed === managed &&
      this.inlineWork === inlineWork &&
      blocks.length >= this.tailStart &&
      blocks[this.tailStart] === this.blocks[this.tailStart] &&
      samePrefix(blocks, this.blocks, this.tailStart);
    const prefix = reusePrefix ? this.turns.slice(0, -1) : [];
    const source = reusePrefix ? blocks.slice(this.tailStart) : blocks;
    const previous = new Map(
      (reusePrefix ? this.turns.slice(-1) : this.turns).map((turn) => [
        turn[0].id,
        turn,
      ]),
    );
    const grouped = inlineWork
      ? groupMonoChatTurns(source, managed)
      : groupTurns(source, managed);
    const next = prefix.concat(
      grouped.map((turn) => {
        const before = previous.get(turn[0].id);
        return before &&
          before.length === turn.length &&
          turn.every((block, index) => block === before[index])
          ? before
          : turn;
      }),
    );
    this.blocks = blocks;
    this.managed = managed;
    this.inlineWork = inlineWork;
    if (
      next.length !== this.turns.length ||
      next.some((turn, index) => turn !== this.turns[index])
    )
      this.turns = next;
    const last = this.turns[this.turns.length - 1];
    this.tailStart = last ? blocks.indexOf(last[0]) : 0;
    return this.turns;
  }

  turnItems(
    turn: Block[],
    settled: boolean,
    {
      managed = false,
      inlineWork = false,
      undeliveredMessageIds,
    }: {
      managed?: boolean;
      inlineWork?: boolean;
      undeliveredMessageIds?: ReadonlySet<string>;
    } = {},
  ): TurnItem[] {
    let variants = this.items.get(turn);
    const undelivered =
      inlineWork && undeliveredMessageIds?.size
        ? turn
            .filter((block) => undeliveredMessageIds?.has(block.id))
            .map((block) => block.id)
        : [];
    const key = JSON.stringify([settled, managed, inlineWork, undelivered]);
    const previous = variants?.get(key);
    if (previous) return previous;
    const visible = (blocks: Block[]) =>
      blocks.filter(
        (block) => !block.orchestration && (managed || !block.internal),
      );
    // Each run in a merged Mono turn folds on its own: an earlier answer is
    // not narration for the work that the next run went on to do.
    const items = inlineWork
      ? monoTurnRuns(turn).flatMap((run, index, runs) =>
          groupMonoTurnItems(visible(run), {
            live: !settled && index === runs.length - 1,
            undeliveredMessageIds,
          }),
        )
      : groupTurnItems(visible(turn), { settled });
    if (!variants) {
      variants = new Map();
      this.items.set(turn, variants);
    }
    variants.set(key, items);
    return items;
  }
}

function samePrefix(next: Block[], previous: Block[], end: number): boolean {
  for (let i = 0; i < end; i++) if (next[i] !== previous[i]) return false;
  return true;
}
