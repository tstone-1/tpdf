import { describe, expect, it } from "vitest";
import { fitSignature, rotateSignature, trimSignature, validSignature, type SignatureImage } from "./signature";

const image: SignatureImage = { width: 2, height: 1, rgba: [255, 0, 0, 255, 0, 0, 255, 128] };
describe("visual signature pixels", () => {
  it("validates pixel lengths, bytes, visibility and allocation limits", () => {
    expect(validSignature(image)).toBe(true);
    for (const value of [null, {}, { ...image, width: 0 }, { ...image, height: 513 }, { ...image, rgba: [1] },
      { ...image, rgba: [256, 0, 0, 255, 0, 0, 0, 255] }, { ...image, rgba: new Array(8).fill(0) },
      { width: 512, height: 512, rgba: new Array(512*512*4).fill(255) }]) expect(validSignature(value)).toBe(false);
  });
  it("removes transparent margins and preserves semi-transparent coloured pixels", () => {
    const pixels = new Uint8ClampedArray(4*3*4); pixels.set(image.rgba, (4+1)*4);
    expect(trimSignature(4, 3, pixels)).toEqual(image);
    expect(trimSignature(4, 3, new Uint8ClampedArray(48))).toBeNull();
  });
  it("fits a wide signature without stretching or escaping the dragged rectangle", () => {
    expect(fitSignature({ left: 10, top: 20, right: 110, bottom: 120 }, image)).toEqual({ left: 10, top: 20, right: 110, bottom: 70 });
  });
  it("rotates pixel orientation with the page and reverses a placement turn", () => {
    expect(rotateSignature(image, 1)).toEqual({ width: 1, height: 2, rgba: image.rgba });
    expect(rotateSignature(image, 2).rgba).toEqual([...image.rgba.slice(4), ...image.rgba.slice(0, 4)]);
    for (let turns = 0; turns < 4; turns++) expect(rotateSignature(rotateSignature(image, -turns), turns)).toEqual(image);
  });
});
