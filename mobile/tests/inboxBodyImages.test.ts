import { beforeEach, describe, expect, it, vi } from "vitest";

import { imageUrls, keptImage, readImage } from "@/features/inbox/bodyImages";

describe("image numbers", () => {
  it("counts as the host's image_urls does", () => {
    // The body of `image_urls_come_back_in_reading_order` in mobile/inbox.rs.
    const body =
      'Intro ![a](https://x.test/1.png) text <img width="3" src="https://x.test/2.png"> ![b](<https://x.test/3.png> "t")';
    expect(imageUrls(body)).toEqual([
      "https://x.test/1.png",
      "https://x.test/2.png",
      "https://x.test/3.png",
    ]);
    expect(imageUrls("no images, just [a link](https://x.test)")).toEqual([]);
  });

  it("counts images the page never shows, as the host does", () => {
    const body = [
      "```md",
      "![example](https://x.test/in-code.png)",
      "```",
      "<!-- <img src='https://x.test/in-comment.png'> -->",
      '<IMG SRC="https://x.test/real.png">',
    ].join("\n");
    expect(imageUrls(body)).toEqual([
      "https://x.test/in-code.png",
      "https://x.test/in-comment.png",
      "https://x.test/real.png",
    ]);
  });

  it("keeps its offsets past text that changes length in lower case", () => {
    expect(imageUrls('İİİ <img src="https://x.test/Case.PNG">')).toEqual([
      "https://x.test/Case.PNG",
    ]);
  });
});

describe("reading images", () => {
  let item = 0;
  let itemKey = "";
  beforeEach(() => {
    item += 1;
    itemKey = `github:o/r#${item}`;
  });
  const media = (base64 = "aGk=") => vi.fn(async () => ({ mime: "image/png", base64 }));

  it("asks by body index, or by comment, never by address", async () => {
    const client = { media: media() };
    await readImage(client, { itemKey }, { src: "https://x.test/a.png", index: 2 });
    expect(client.media).toHaveBeenLastCalledWith(itemKey, { index: 2 });
    await readImage(
      client,
      { itemKey, commentId: "c1" },
      { src: "https://x.test/a.png", index: 0 },
    );
    expect(client.media).toHaveBeenLastCalledWith(itemKey, { commentId: "c1", commentIndex: 0 });
  });

  it("reads an image once and keeps it for the next open", async () => {
    const client = { media: media() };
    const image = { src: "https://x.test/a.png", index: 0 };
    const [first, second] = await Promise.all([
      readImage(client, { itemKey }, image),
      readImage(client, { itemKey }, image),
    ]);
    expect(first).toBe("data:image/png;base64,aGk=");
    expect(second).toBe(first);
    await readImage(client, { itemKey }, image);
    expect(client.media).toHaveBeenCalledTimes(1);
    expect(keptImage({ itemKey }, image.src)).toBe(first);
  });

  it("tries a failed image again", async () => {
    const client = {
      media: vi
        .fn()
        .mockRejectedValueOnce(new Error("offline"))
        .mockResolvedValueOnce({ mime: "image/png", base64: "aGk=" }),
    };
    const image = { src: "https://x.test/a.png", index: 0 };
    await expect(readImage(client, { itemKey }, image)).rejects.toThrow("offline");
    await expect(readImage(client, { itemKey }, image)).resolves.toContain("aGk=");
  });

  it("holds a body full of images to a few reads at once", async () => {
    let now = 0;
    let most = 0;
    const client = {
      media: vi.fn(async () => {
        now += 1;
        most = Math.max(most, now);
        await new Promise((resolve) => setTimeout(resolve, 1));
        now -= 1;
        return { mime: "image/png", base64: "aGk=" };
      }),
    };
    await Promise.all(
      Array.from({ length: 9 }, (_, index) =>
        readImage(client, { itemKey }, { src: `https://x.test/${index}.png`, index }),
      ),
    );
    expect(client.media).toHaveBeenCalledTimes(9);
    expect(most).toBe(3);
  });

  it("lets go of the oldest pictures", async () => {
    const client = { media: media() };
    for (let index = 0; index < 13; index += 1)
      await readImage(client, { itemKey }, { src: `https://x.test/${index}.png`, index });
    expect(keptImage({ itemKey }, "https://x.test/0.png")).toBeUndefined();
    expect(keptImage({ itemKey }, "https://x.test/12.png")).toBeDefined();
  });
});
