/**
 * Shared background tone for popup-picker rows (`ModelPicker`, `BranchPicker`,
 * `CwdPicker`, `AccessPicker`, `ModelSettings`).
 *
 * Selection (the current value) keeps the strong wash; focus (keyboard index /
 * mouse hover) uses a lighter one so the two are never confused.
 */
export function pickerRowTone({
  highlighted = false,
  selected = false,
}: {
  highlighted?: boolean;
  selected?: boolean;
}): string {
  if (selected) return "bg-content/10";
  if (highlighted) return "bg-content/5";
  return "hover:bg-content/5";
}
