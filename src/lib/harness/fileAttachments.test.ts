import { describe, expect, it } from "vitest";
import type { Attachment } from "../session";
import { attachmentPathText, promptBlocks } from "../attachments";
import { buildClaudeUserMessage } from "./claudeProtocol";
import { buildPiPrompt, buildPiSteer } from "./piProtocol";
import { grokPromptBlocks } from "./grokProtocol";
import { toOpenCodeFileParts } from "./opencodeProtocol";

const document: Attachment = {
  id: "document",
  name: "report.pdf",
  mimeType: "application/pdf",
  kind: "file",
  size: 100,
  path: "/tmp/report.pdf",
};

const image: Attachment = {
  id: "image",
  name: "screenshot.png",
  mimeType: "image/png",
  kind: "image",
  size: 3,
  data: "YWJj",
  path: "/tmp/screenshot.png",
};

function claudeContent(text: string, attachments: Attachment[]) {
  const result = buildClaudeUserMessage({ text, attachments });
  return (result.message as { content: unknown[] }).content;
}

describe("native file attachment formats", () => {
  it.each([
    ["Cursor ACP", promptBlocks],
    ["Grok ACP", grokPromptBlocks],
  ] as const)("keeps %s documents as resource links", (_name, build) => {
    expect(build("", [document])).toEqual([
      {
        type: "resource_link",
        uri: "file:///tmp/report.pdf",
        name: "report.pdf",
        mimeType: "application/pdf",
        size: 100,
      },
    ]);
    expect(build("look", [image])[1]).toMatchObject({
      type: "image",
      mimeType: "image/png",
      data: "YWJj",
    });
  });

  it("keeps OpenCode's native file parts for documents and pasted data", () => {
    expect(
      toOpenCodeFileParts([document, { ...image, path: undefined }]),
    ).toEqual([
      {
        type: "file",
        mime: "application/pdf",
        filename: "report.pdf",
        url: "file:///tmp/report.pdf",
      },
      {
        type: "file",
        mime: "image/png",
        filename: "screenshot.png",
        url: "data:image/png;base64,YWJj",
      },
    ]);
  });
});

describe("file paths in native harness prompts", () => {
  it.each([
    ["report.pdf", "application/pdf", "file"],
    ["transcript.md", "text/markdown", "file"],
    ["server.log", "text/plain", "file"],
    ["recording.wav", "audio/wav", "audio"],
    ["archive.zip", "application/zip", "file"],
  ] as const)(
    "delivers %s in Claude and Pi/omp prompts and steering",
    (name, mimeType, kind) => {
      const file = { ...document, name, mimeType, kind, path: `/tmp/${name}` };
      const expected = `Attached file (read from disk): ${JSON.stringify(file.path)}`;
      expect(claudeContent("", [file])).toEqual([
        { type: "text", text: expected },
      ]);
      for (const build of [buildPiPrompt, buildPiSteer]) {
        expect(build({ text: "", attachments: [file] })).toMatchObject({
          message: expected,
        });
        expect(build({ text: "Review", attachments: [file] })).toMatchObject({
          message: `Review\n\n${expected}`,
        });
      }
    },
  );

  it("preserves native images alongside documents without duplicate path inputs", () => {
    expect(claudeContent("Review", [image, document])).toEqual([
      { type: "text", text: "Review" },
      {
        type: "image",
        source: { type: "base64", media_type: "image/png", data: "YWJj" },
      },
      {
        type: "text",
        text: 'Attached file (read from disk): "/tmp/report.pdf"',
      },
    ]);
    for (const build of [buildPiPrompt, buildPiSteer]) {
      expect(
        build({ text: "Review", attachments: [image, document] }),
      ).toMatchObject({
        message: 'Review\n\nAttached file (read from disk): "/tmp/report.pdf"',
        images: [{ type: "image", mimeType: "image/png", data: "YWJj" }],
      });
    }
  });

  it.each([
    { ...image, data: undefined, size: 21 * 1024 * 1024 },
    {
      ...image,
      name: "drawing.svg",
      mimeType: "image/svg+xml",
      path: "/tmp/drawing.svg",
    },
  ])("falls back for an image that cannot be embedded: $name", (file) => {
    expect(claudeContent("", [file])).toEqual([
      { type: "text", text: attachmentPathText(file) },
    ]);
    for (const build of [buildPiPrompt, buildPiSteer]) {
      expect(build({ text: "", attachments: [file] })).toMatchObject({
        message: attachmentPathText(file),
      });
      expect(build({ text: "", attachments: [file] }).images).toBeUndefined();
    }
  });

  it.each([
    '/tmp/Quarterly "report" #1.pdf',
    "C:\\Users\\User Name\\report.pdf",
    "/tmp/東京\nreport.pdf",
  ])("quotes the exact path including special characters: %s", (path) => {
    const text = attachmentPathText({ ...document, path });
    expect(JSON.parse(text.slice(text.indexOf(": ") + 2))).toBe(path);
  });

  it("reports a missing source instead of silently dropping an attachment", () => {
    const files = [{ ...document, path: undefined }];
    const builders = [
      () => claudeContent("Review", files),
      () => buildPiPrompt({ text: "Review", attachments: files }),
      () => buildPiSteer({ text: "Review", attachments: files }),
      () => promptBlocks("Review", files),
      () => toOpenCodeFileParts(files),
    ];
    for (const build of builders)
      expect(build).toThrow(/report\.pdf.*no local file path/);
  });
});
