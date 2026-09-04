import { describe, expect, it } from "vitest";
import { formatFileSize, isImagePath, sniffImageMime } from "./filePreview";

function bytes(...values: number[]): Uint8Array {
  return new Uint8Array(values);
}

describe("isImagePath", () => {
  it("routes images away from the editor", () => {
    expect(isImagePath("/w/shot.png")).toBe(true);
    expect(isImagePath("/w/Photo.JPEG")).toBe(true);
    expect(isImagePath("/w/icon.webp")).toBe(true);
  });

  it("leaves text, svg and extensionless files to the editor", () => {
    expect(isImagePath("/w/main.rs")).toBe(false);
    expect(isImagePath("/w/logo.svg")).toBe(false);
    expect(isImagePath("/w/spec.pdf")).toBe(false);
    expect(isImagePath("/w/LICENSE")).toBe(false);
    expect(isImagePath("/w/.png/notes.txt")).toBe(false);
  });
});

describe("sniffImageMime", () => {
  it("identifies each supported format by magic number", () => {
    expect(sniffImageMime(bytes(0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a))).toBe(
      "image/png",
    );
    expect(sniffImageMime(bytes(0xff, 0xd8, 0xff, 0xe0))).toBe("image/jpeg");
    expect(sniffImageMime(bytes(0x47, 0x49, 0x46, 0x38, 0x39, 0x61))).toBe(
      "image/gif",
    );
    expect(
      sniffImageMime(
        bytes(0x52, 0x49, 0x46, 0x46, 1, 2, 3, 4, 0x57, 0x45, 0x42, 0x50),
      ),
    ).toBe("image/webp");
    expect(
      sniffImageMime(
        bytes(0, 0, 0, 0x20, 0x66, 0x74, 0x79, 0x70, 0x61, 0x76, 0x69, 0x66),
      ),
    ).toBe("image/avif");
  });

  it("refuses content that is not a supported image", () => {
    const html = new TextEncoder().encode("<html><script>x()</script>");
    expect(sniffImageMime(html)).toBeNull();
    expect(sniffImageMime(new TextEncoder().encode("%PDF-1.7"))).toBeNull();
    expect(sniffImageMime(bytes(0x89, 0x50))).toBeNull();
    expect(sniffImageMime(bytes())).toBeNull();
    // An MP4 shares the ftyp box with AVIF but is not an image.
    expect(
      sniffImageMime(
        bytes(0, 0, 0, 0x20, 0x66, 0x74, 0x79, 0x70, 0x69, 0x73, 0x6f, 0x6d),
      ),
    ).toBeNull();
  });
});

describe("formatFileSize", () => {
  it("scales to the largest unit that keeps a leading digit", () => {
    expect(formatFileSize(0)).toBe("0 B");
    expect(formatFileSize(900)).toBe("900 B");
    expect(formatFileSize(2048)).toBe("2.0 KB");
    expect(formatFileSize(48 * 1024)).toBe("48 KB");
    expect(formatFileSize(3 * 1024 * 1024)).toBe("3.0 MB");
    expect(formatFileSize(2 * 1024 * 1024 * 1024)).toBe("2.0 GB");
  });
});
