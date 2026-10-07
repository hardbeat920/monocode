import type { Block } from "./session";
import {
  groupMonoTurnItems,
  groupMonoTurns,
  groupTurnItems,
  groupTurns,
  type TurnItem,
} from "./transcriptActivity";

/** Reuse unchanged history while immutable updates replace the live blocks. */
export class TranscriptTurnCache {
  private blocks: Block[] | undefined;
  private managed = false;
  private inlineWork = false;
  private turns: Block[][] = [];
  private items = new WeakMap<Block[], Map<string, TurnItem[]>>();

  group(blocks: Block[], managed = false, inlineWork = false): Block[][] {
    if (
      this.blocks === blocks &&
      this.managed === managed &&
      this.inlineWork === inlineWork
    )
      return this.turns;
    const previous = new Map(this.turns.map((turn) => [turn[0].id, turn]));
    const grouped = inlineWork
      ? groupMonoTurns(blocks, managed)
      : groupTurns(blocks, managed);
    const next = grouped.map((turn) => {
      const before = previous.get(turn[0].id);
      return before &&
        before.length === turn.length &&
        turn.every((block, index) => block === before[index])
        ? before
        : turn;
    });
    this.blocks = blocks;
    this.managed = managed;
    this.inlineWork = inlineWork;
    if (
      next.length !== this.turns.length ||
      next.some((turn, index) => turn !== this.turns[index])
    )
      this.turns = next;
    return this.turns;
  }

  turnItems(
    turn: Block[],
    settled: boolean,
    { managed = false, inlineWork = false } = {},
  ): TurnItem[] {
    let variants = this.items.get(turn);
    const key = `${settled}/${managed}/${inlineWork}`;
    const previous = variants?.get(key);
    if (previous) return previous;
    const blocks = turn.filter(
      (block) => !block.orchestration && (managed || !block.internal),
    );
    const items = inlineWork
      ? groupMonoTurnItems(blocks, { live: !settled })
      : groupTurnItems(blocks, { settled });
    if (!variants) {
      variants = new Map();
      this.items.set(turn, variants);
    }
    variants.set(key, items);
    return items;
  }
}
