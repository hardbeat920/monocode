/**
 * A catalog entry is either a plain message or a pair of plural forms.
 * Turkish uses the same form for both, so its `one` and `other` match.
 */
export type MessageValue = string | { readonly one: string; readonly other: string };

/**
 * The shape a locale must provide for every key in the English source
 * catalog. Typing a catalog against `Translations<typeof en>` makes a
 * missing or mis-shaped key a compile error instead of a runtime fallback.
 */
export type Translations<T> = {
  [K in keyof T]: T[K] extends string ? string : { one: string; other: string };
};
