import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { escapeNonMathDollars } from "./agentMath";
import { AgentMarkdown } from "./AgentMarkdown";

describe("AgentMarkdown text direction", () => {
  it("detects direction independently for RTL and LTR blocks", () => {
    const markup = renderToStaticMarkup(
      createElement(AgentMarkdown, {
        text: [
          "# راهنمای تنظیمات",
          "",
          "این متن فارسی است.",
          "",
          "1. مرحله اول",
          "2. مرحله دوم",
          "",
          "English remains left to right.",
        ].join("\n"),
      }),
    );

    expect(markup).toMatch(/dir="rtl"[^>]*><h1/);
    expect(markup).toMatch(/dir="rtl"[^>]*><p/);
    expect(markup).toMatch(/dir="rtl"[^>]*><ol/);
    expect(markup).toMatch(/dir="ltr"[^>]*><p/);
  });

  it("isolates links and inline code inside RTL prose", () => {
    const markup = renderToStaticMarkup(
      createElement(AgentMarkdown, {
        text: "مسیر `templates/admin/settings.html` و [پیوند](https://example.com) را بررسی کنید.",
      }),
    );

    expect(markup).toContain('<code dir="ltr"');
    expect(markup).toContain('dir="auto"');
  });

  it("keeps fenced code blocks LTR when their content is Arabic", () => {
    const markup = renderToStaticMarkup(
      createElement(AgentMarkdown, {
        text: "```txt\nمرحبا بالعالم\n```",
      }),
    );

    expect(markup).toContain('class="markdown-code-shell" dir="ltr"');
  });
});

describe("AgentMarkdown inline code", () => {
  it("lets a long inline code span grow instead of clipping to a fixed height", () => {
    const markup = renderToStaticMarkup(
      createElement(AgentMarkdown, {
        text: "1. `Before I refactor the transcript rendering, summarize how turns are grouped, in five bullets.`",
      }),
    );

    const match = markup.match(/<code dir="ltr" class="([^"]*)"/);
    expect(match).not.toBeNull();
    const classes = match![1].split(/\s+/);
    expect(classes).toContain("inline-flex");
    expect(classes).toContain("min-h-6");
    expect(classes).toContain("max-w-full");
    expect(classes).toContain("[overflow-wrap:anywhere]");
    expect(classes).not.toContain("h-6");
  });
});

describe("AgentMarkdown code fence highlighting", () => {
  it("falls back to JS highlighting for a fence tagged text", () => {
    const markup = renderToStaticMarkup(
      createElement(AgentMarkdown, {
        text: "```text\nconst x = 1;\n```",
      }),
    );

    expect(markup).toContain('data-language="js"');
    expect(markup).not.toContain('data-language="text"');
  });

  it("falls back to JS highlighting for an untagged fence", () => {
    const markup = renderToStaticMarkup(
      createElement(AgentMarkdown, {
        text: "```\nconst x = 1;\n```",
      }),
    );

    expect(markup).toContain('data-language="js"');
  });

  it("leaves an explicit real language alone", () => {
    const markup = renderToStaticMarkup(
      createElement(AgentMarkdown, {
        text: "```python\nx = 1\n```",
      }),
    );

    expect(markup).toContain('data-language="python"');
  });

  it("keeps a text fence labeled text despite the JS highlighting fallback", () => {
    const markup = renderToStaticMarkup(
      createElement(AgentMarkdown, {
        text: "```text\nconst x = 1;\n```",
      }),
    );

    expect(markup).toContain('data-language="js"');
    expect(markup).toMatch(
      /class="markdown-code-fallback-label"[^>]*>text<\/span>/,
    );
  });

  it("leaves an untagged fence without a visible language label", () => {
    const markup = renderToStaticMarkup(
      createElement(AgentMarkdown, {
        text: "```\nconst x = 1;\n```",
      }),
    );

    expect(markup).toMatch(
      /class="markdown-code-fallback-label"[^>]*><\/span>/,
    );
  });

  it("keeps the plaintext and txt fallbacks labeled with their own fence language", () => {
    const plaintextMarkup = renderToStaticMarkup(
      createElement(AgentMarkdown, {
        text: "```plaintext\nconst x = 1;\n```",
      }),
    );
    expect(plaintextMarkup).toContain('data-language="js"');
    expect(plaintextMarkup).toMatch(
      /class="markdown-code-fallback-label"[^>]*>plaintext<\/span>/,
    );

    const txtMarkup = renderToStaticMarkup(
      createElement(AgentMarkdown, {
        text: "```txt\nconst x = 1;\n```",
      }),
    );
    expect(txtMarkup).toContain('data-language="js"');
    expect(txtMarkup).toMatch(
      /class="markdown-code-fallback-label"[^>]*>txt<\/span>/,
    );
  });

  it("does not add a fallback label for a real language", () => {
    const markup = renderToStaticMarkup(
      createElement(AgentMarkdown, {
        text: "```python\nx = 1\n```",
      }),
    );

    expect(markup).not.toContain("markdown-code-fallback-label");
  });
});

describe("AgentMarkdown note images", () => {
  it("keeps app-owned note image references for the async image resolver", () => {
    const markup = renderToStaticMarkup(
      createElement(AgentMarkdown, {
        text: "![Diagram](/note-assets/note-1/123-diagram.png)",
      }),
    );

    expect(markup).toContain(
      'data-note-image="/note-assets/note-1/123-diagram.png"',
    );
    expect(markup).toContain('alt="Diagram"');
  });
});

describe("AgentMarkdown math", () => {
  const render = (text: string, streaming = false) =>
    renderToStaticMarkup(createElement(AgentMarkdown, { text, streaming }));

  it("typesets inline $...$ math with KaTeX", () => {
    const markup = render("Energy is $E = mc^2$ here.");

    expect(markup).toContain('class="katex"');
    expect(markup).not.toContain("$E = mc^2$");
  });

  it("typesets $$...$$ blocks as display math", () => {
    const markup = render("$$\n\\int_0^1 x^2\\,dx = \\frac{1}{3}\n$$");

    expect(markup).toContain("katex-display");
    expect(markup).toContain("<math");
  });

  it("typesets math while a reply is still streaming", () => {
    const markup = render("So $a^2 + b^2 = c^2$ holds.", true);

    expect(markup).toContain('class="katex"');
  });

  it("leaves dollar amounts as plain text", () => {
    const markup = render("It costs $5 and $10 per seat.");

    expect(markup).not.toContain("katex");
    expect(markup).toContain("$5 and $10");
  });

  it("keeps markdown formatting between two dollar amounts", () => {
    const markup = render(
      "Pay $5 for `npm ci` and **fast** builds, $10 otherwise.",
    );

    expect(markup).not.toContain("katex");
    expect(markup).toContain(">npm ci</code>");
    expect(markup).toContain('data-streamdown="strong">fast<');
    expect(markup).not.toContain("**");
    expect(markup).toContain("$5 for ");
  });

  it("does not typeset a math span followed by a digit", () => {
    const markup = render("Between $x$5 and more.");

    expect(markup).not.toContain("katex");
  });

  it("shows oversized math as code instead of typesetting it", () => {
    const huge = "x".repeat(6000);
    const markup = render(`Inline $${huge}$ and\n\n$$\n${huge}\n$$`);

    expect(markup).not.toContain("katex");
    expect(markup).toContain(huge);
  });

  it("typesets a fenced math block", () => {
    const markup = render("```math\n\\frac{1}{2}\n```");

    expect(markup).toContain("katex-display");
  });

  it("does not typeset an oversized fenced math block", () => {
    const huge = "x".repeat(6000);
    const markup = render(`\`\`\`math\n${huge}\n\`\`\``);

    expect(markup).not.toContain("katex");
    expect(markup).toContain(huge);
  });

  it("does not typeset oversized raw language-math HTML", () => {
    const huge = "x".repeat(6000);
    const markup = render(`<code class="language-math">${huge}</code>`);

    expect(markup).not.toContain("katex");
    expect(markup).toContain(huge);
  });

  it("measures the whole pre that KaTeX would typeset", () => {
    const huge = "x".repeat(6000);
    const markup = render(
      `<pre>${huge}<code class="language-math">y</code></pre>`,
    );

    expect(markup).not.toContain("katex");
  });

  it("caps how much math one block typesets", () => {
    const markup = render(Array.from({ length: 600 }, () => "$x$").join(" "));

    expect(markup.match(/class="katex"/g)).toHaveLength(500);
  });

  it("leaves dollars inside bare links alone", () => {
    const markup = render("See https://example.com/?q=$a and $b$ here.");

    expect(markup).toContain('href="https://example.com/?q=$a"');
    expect(markup.match(/class="katex"/g)).toHaveLength(1);
    expect(render("See <https://example.com/?q=$x> for $5")).toContain(
      'href="https://example.com/?q=$x"',
    );
  });

  it("keeps markdown that spans a dollar amount", () => {
    const markup = render("Pay $5 **or $10** today.");

    expect(markup).not.toContain("katex");
    expect(markup).toContain('data-streamdown="strong">or $10<');
    expect(markup).toContain("Pay $5 ");
  });

  it("still typesets math that follows a dollar amount", () => {
    const markup = render("It costs $5. Use $x^2$ to compute it.");

    expect(markup.match(/class="katex"/g)).toHaveLength(1);
    expect(markup).toContain("It costs $5. Use ");
  });

  it("pairs math inside emphasis after a bold price", () => {
    const markup = render("**$5** or *$x$*");

    expect(markup.match(/class="katex"/g)).toHaveLength(1);
    expect(markup).toContain('data-streamdown="strong">$5<');
  });
});

describe("escapeNonMathDollars", () => {
  it("leaves display math source untouched", () => {
    const text = "$$\nx + $y\n$$";

    expect(escapeNonMathDollars(text)).toBe(text);
  });

  it("leaves dollars in code untouched", () => {
    const text = "Run `echo $HOME` then\n\n```sh\necho $PATH $5\n```";

    expect(escapeNonMathDollars(text)).toBe(text);
  });
});
