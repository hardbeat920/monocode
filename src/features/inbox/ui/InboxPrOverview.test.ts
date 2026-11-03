import { describe, expect, it } from "vitest";
import { descriptionExcerpt } from "./InboxPrOverview";

describe("descriptionExcerpt", () => {
  it("keeps short plain descriptions whole", () => {
    expect(descriptionExcerpt("Fixes the retry loop.")).toEqual({
      text: "Fixes the retry loop.",
      images: 0,
      truncated: false,
    });
  });

  it("strips markdown and media, counting images for the expand hint", () => {
    const excerpt = descriptionExcerpt(
      [
        "<!-- template -->",
        "## Summary",
        "- Adds an **idempotency** key ([ENG-142](https://x.dev))",
        "![shot](https://x.dev/a.png)",
        '<img src="https://x.dev/b.png">',
        "```ts",
        "const hidden = true;",
        "```",
      ].join("\n"),
    );
    expect(excerpt.text).toBe("Summary\nAdds an idempotency key (ENG-142)");
    expect(excerpt.images).toBe(2);
    expect(excerpt.truncated).toBe(true);
  });

  it("collapses long bodies to the lead lines", () => {
    const excerpt = descriptionExcerpt("a\nb\nc\nd\ne\nf");
    expect(excerpt.text).toBe("a\nb\nc\nd");
    expect(excerpt.truncated).toBe(true);
  });
});
