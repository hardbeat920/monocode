const locales = ["en", "fr", "ja", "ar"];
console.log(JSON.stringify({node: process.version, icu: process.versions.icu, defaultLocale: new Intl.Collator().resolvedOptions().locale}));
for (const locale of locales) {
  const relative = new Intl.RelativeTimeFormat(locale, {numeric: "auto"});
  const cases = [[0,"second"],[-1,"day"],[1,"day"],[-2,"day"],[2,"day"],[-2,"hour"],[2,"hour"],[-59,"second"],[1,"minute"],[-4,"week"],[1,"month"]];
  console.log(JSON.stringify({locale, relative: cases.map(([value, unit]) => ({value, unit, text: relative.format(value, unit)})), punctuation: ["file.a.rs", "file-a.rs", "file_a.rs"].sort((a,b) => a.localeCompare(b,locale)), accents: ["filez.rs", "fileé.rs", "filee.rs"].sort((a,b) => a.localeCompare(b,locale)), canonicalEquivalence: [["fileé.rs","filee\u0301.rs"],["fileÅ.rs","fileÅ.rs"]].map(([a,b])=>({a,b,comparison:a.localeCompare(b,locale)}))}));
}
