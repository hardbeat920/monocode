type ImeKeyboardEvent = Pick<KeyboardEvent, "isComposing" | "keyCode">;

export function isImeComposition(event: ImeKeyboardEvent): boolean {
  return event.isComposing || event.keyCode === 229;
}
