const relativeLocales = ["fr-FR", "fr-u-nu-foo", "fr-u-nu-roman", "fr-u-nu-native", "cmn", "und", "und-u-nu-arab", "und-Latn", "und-US", "und-Cyrl"];
const comparisonCases = [
  ["en-u-kn-foo", "9", "10"],
  ["en-u-kf-yes", "a", "A"],
  ["en-u-kn-yes", "9", "10"],
  ["en-u-ka-shifted", "a-b", "ab"],
  ["en-u-kc-true", "a", "A"],
  ["en-u-ks-level1", "é", "e"],
  ["de-DE-u-co-search", "ä", "ae"],
  ["ja-JP-u-co-search", "ひ", "ヒ"],
  ["en-US-u-co-search-kn-true", "9", "10"],
  ["ccp", "ä", "z"],
];
const result = {
  node: process.version,
  icu: process.versions.icu,
  environment: { LC_ALL: process.env.LC_ALL, LANG: process.env.LANG },
  defaultCollator: new Intl.Collator().resolvedOptions(),
  relative: relativeLocales.map(locale => {
    const formatter = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
    return { requested: locale, resolved: formatter.resolvedOptions(), phrase: formatter.format(-3, "hour") };
  }),
  comparisons: comparisonCases.map(([locale, a, b]) => {
    const collator = new Intl.Collator(locale);
    return { requested: locale, resolved: collator.resolvedOptions(), a, b, order: Math.sign(collator.compare(a, b)) };
  }),
};
console.log(JSON.stringify(result));
