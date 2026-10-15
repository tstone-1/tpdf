/**
 * The bytes a picture travels as, its turns, and the overlay's copy of each.
 */

import { describe, expect, it } from "vitest";

import { PICTURE_PIXELS, PICTURE_SIDE, PictureCache, pictureBody, readPicture, rotatePixels, visible, type PicturePixels } from "./picture";

/** A reply as `picture_reply` in `commands/edit.rs` lays it out. */
function reply(width: number, height: number, rgba: number[]): ArrayBuffer {
  const out = new Uint8Array(8 + rgba.length);
  const view = new DataView(out.buffer);
  view.setUint32(0, width, true);
  view.setUint32(4, height, true);
  out.set(rgba, 8);
  return out.buffer;
}

/** Pixels whose red channel numbers them, so a turn can be read back. */
function numbered(width: number, height: number): PicturePixels {
  const rgba = new Uint8Array(width * height * 4);
  for (let n = 0; n < width * height; n++) rgba.set([n, 0, 0, 255], n * 4);
  return { width, height, rgba };
}

const reds = (pixels: PicturePixels) => [...pixels.rgba].filter((_, i) => i % 4 === 0);

describe("readPicture", () => {
  it("reads the size and the pixels after it", () => {
    const pixels = readPicture(reply(2, 1, [1, 2, 3, 255, 4, 5, 6, 0]));
    expect([pixels.width, pixels.height]).toEqual([2, 1]);
    expect([...pixels.rgba]).toEqual([1, 2, 3, 255, 4, 5, 6, 0]);
  });

  it("refuses a reply whose parts do not add up or that is past a placed picture's limits", () => {
    expect(() => readPicture(reply(2, 1, [1, 2, 3, 255]))).toThrow();
    expect(() => readPicture(reply(1, 1, [1, 2, 3, 255, 0]))).toThrow();
    expect(() => readPicture(reply(0, 1, []))).toThrow();
    expect(() => readPicture(new ArrayBuffer(7))).toThrow();
    // Sizes past the limits, each with the pixels it states, so the length
    // check above cannot be what refuses them. The control is one row less.
    const full = (width: number, height: number) => reply(width, height, new Array<number>(width * height * 4).fill(255));
    expect(() => readPicture(full(PICTURE_SIDE + 1, 1))).toThrow();
    expect(() => readPicture(full(2048, PICTURE_PIXELS / 2048 + 1))).toThrow();
    expect(readPicture(full(2048, PICTURE_PIXELS / 2048)).height).toBe(PICTURE_PIXELS / 2048);
  });
});

describe("pictureBody", () => {
  it("is the JSON's length, the JSON and the pixels, as picture.rs reads them", () => {
    const pixels = numbered(2, 1);
    const body = pictureBody({ doc: 3 }, pixels);
    const length = new DataView(body.buffer).getUint32(0, true);
    const json = new TextDecoder().decode(body.subarray(4, 4 + length));
    expect(JSON.parse(json)).toEqual({ doc: 3 });
    expect([...body.subarray(4 + length)]).toEqual([...pixels.rgba]);
  });

  it("counts the JSON's length in bytes, not characters", () => {
    const body = pictureBody({ note: "Grüße" }, numbered(1, 1));
    const length = new DataView(body.buffer).getUint32(0, true);
    expect(length).toBe(new TextEncoder().encode(JSON.stringify({ note: "Grüße" })).length);
    expect(body.length).toBe(4 + length + 4);
  });
});

describe("rotatePixels", () => {
  //  0 1 2      a quarter turn clockwise     3 0
  //  3 4 5                                  4 1
  //                                          5 2
  it("turns a quarter clockwise per turn, and back by the negative", () => {
    const source = numbered(3, 2);
    const once = rotatePixels(source, 1);
    expect([once.width, once.height]).toEqual([2, 3]);
    expect(reds(once)).toEqual([3, 0, 4, 1, 5, 2]);
    expect(reds(rotatePixels(source, 2))).toEqual([5, 4, 3, 2, 1, 0]);
    expect(reds(rotatePixels(once, -1))).toEqual(reds(source));
    expect(rotatePixels(source, 4)).toBe(source);
  });

  it("reads pixels whose view does not start on a word", () => {
    const backing = new Uint8Array(1 + 8);
    backing.set([0, 1, 0, 0, 255, 2, 0, 0, 255]);
    const pixels = { width: 2, height: 1, rgba: backing.subarray(1) };
    expect(reds(rotatePixels(pixels, 1))).toEqual([1, 2]);
  });
});

describe("visible", () => {
  it("is false only when every pixel is transparent", () => {
    expect(visible({ width: 1, height: 1, rgba: new Uint8Array([9, 9, 9, 0]) })).toBe(false);
    expect(visible({ width: 2, height: 1, rgba: new Uint8Array([9, 9, 9, 0, 0, 0, 0, 1]) })).toBe(true);
  });
});

describe("PictureCache", () => {
  it("fetches a mark once, answers it when it lands, and asks again after a failure", async () => {
    const asked: number[] = [];
    let readied = 0;
    let fail = true;
    const cache = new PictureCache<string>(async (mark) => {
      asked.push(mark);
      if (mark === 2 && fail) throw new Error("gone");
      return `pixels of ${mark}`;
    }, () => { readied++; });
    expect(cache.get(1)).toBeNull();
    expect(cache.get(1)).toBeNull();
    await Promise.resolve(); await Promise.resolve();
    expect(cache.get(1)).toBe("pixels of 1");
    expect(asked).toEqual([1]);
    expect(readied).toBe(1);
    // A failed fetch is forgotten, so the next paint asks again.
    expect(cache.get(2)).toBeNull();
    await Promise.resolve(); await Promise.resolve();
    fail = false;
    expect(cache.get(2)).toBeNull();
    await Promise.resolve(); await Promise.resolve();
    expect(cache.get(2)).toBe("pixels of 2");
    expect(asked).toEqual([1, 2, 2]);
  });

  it("drops what is not kept, and a fetch that lands after its mark was dropped", async () => {
    const cache = new PictureCache<string>(async (mark) => `pixels of ${mark}`, () => {});
    cache.get(1);
    cache.get(2);
    cache.keep(new Set([1]));
    await Promise.resolve(); await Promise.resolve();
    expect(cache.get(1)).toBe("pixels of 1");
    // Mark 2 was dropped while it was fetched, so it is fetched afresh.
    expect(cache.get(2)).toBeNull();
  });
});
