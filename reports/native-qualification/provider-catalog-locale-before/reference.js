const names = ["\u00e9clair", "e\u0301clair", "Zebra"];
const input = ["Zebra", "\u00e9clair", "e\u0301clair", "xhigh", "high"];
const ranks = ["none", "minimal", "low", "medium", "high", "xhigh", "extra-high", "max", "ultra"];
const rank = value => {
  const index = ranks.indexOf(value.toLowerCase());
  return index < 0 ? Number.MAX_SAFE_INTEGER : index;
};
const value = {
  node: process.version,
  locale: new Intl.Collator().resolvedOptions().locale,
  models: names.slice().sort((left, right) => left.localeCompare(right)),
  variants: input.slice().sort((left, right) => rank(left) - rank(right) || left.localeCompare(right)),
};
console.log(JSON.stringify(value, null, 2).replace(/[\u0080-\uffff]/g,
  character => "\\u" + character.charCodeAt(0).toString(16).padStart(4, "0")));
