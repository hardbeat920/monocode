/**
 * A `;` ends a statement in a sequence diagram, so a message like
 * `A->>B: load; check` fails to parse. Models write that often. Escaping
 * the semicolons after a statement's `:` keeps the text and lets it render.
 * Entities Mermaid recognizes (`#\w+;`, such as `#59;`, `#quot;` or
 * `#frac12;`) are left whole, so repairing twice changes nothing.
 * YAML frontmatter is kept as written; only the diagram after it is repaired.
 * Returns the source unchanged when there is nothing to repair.
 */
export function repairMermaid(source: string): string {
  const frontmatter = /^\s*---\r?\n[\s\S]*?\r?\n---[^\S\r\n]*(?:\r?\n|$)/.exec(
    source,
  )?.[0];
  const body = frontmatter ? source.slice(frontmatter.length) : source;
  if (!/^\s*sequenceDiagram\b/.test(body)) return source;
  const repaired = body
    .split("\n")
    .map((line) => {
      const colon = line.indexOf(":");
      if (colon < 0 || !line.includes(";", colon)) return line;
      return (
        line.slice(0, colon + 1) +
        line
          .slice(colon + 1)
          .replace(/(#\w+;)|;(?!\s*$)/g, (match, entity) =>
            entity ? match : "#59;",
          )
      );
    })
    .join("\n");
  return (frontmatter ?? "") + repaired;
}
