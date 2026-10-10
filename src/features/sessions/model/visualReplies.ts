/**
 * Nothing tells an agent the transcript can draw, so it almost never does.
 * This rides along once per provider session, ahead of the first reply.
 */
export const VISUAL_REPLIES_CONTEXT = [
  "<monocode_visuals>",
  "Your replies render as Markdown in MonoCode, and ```mermaid fences render as diagrams styled to match the app.",
  "Whenever an answer explains how parts connect or how something flows (a request path, a lifecycle, an architecture, a data model, a schedule, or numbers worth comparing), include a small diagram, even when the user did not ask for one and even when they asked for a brief answer: a few nodes plus a few lines of text is briefer than paragraphs.",
  "Pick the type that fits: a flowchart or sequence diagram for a process, a state or class diagram for structure, an ER diagram for data, a gantt or timeline for a schedule, and pie, xychart-beta or quadrantChart for numbers.",
  "Keep each diagram small and focused, give it a short sentence of context, and leave styling (colors, classDef, %%{init}%%) to the app.",
  "Diagrams that fail to parse show as raw source, so keep labels plain: no semicolons in sequence messages, and quote node labels that contain brackets, parentheses or punctuation.",
  "Skip diagrams for simple facts, yes/no answers, and code edits.",
  "</monocode_visuals>",
].join("\n");
