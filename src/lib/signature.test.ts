import { describe, expect, it, vi } from "vitest";
import { decodeSignature, signatureDimensions, fitSignature, rotateSignature, trimSignature, validSignature, type SignatureImage } from "./signature";

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


// Header-only fixtures are intentionally not decodable images. The spy proves
// the allocation boundary runs before a browser decoder receives these bytes.
function png(width: number, height: number, animated = false): Uint8Array<ArrayBuffer> {
  const bytes = new Uint8Array(animated ? 69 : 57);
  bytes.set([137,80,78,71,13,10,26,10]);
  const view = new DataView(bytes.buffer);
  view.setUint32(8, 13); view.setUint32(12, 0x49484452);
  view.setUint32(16, width); view.setUint32(20, height);
  view.setUint32(37, animated ? 0x6163544c : 0x49444154);
  if (animated) view.setUint32(49, 0x49444154);
  view.setUint32(bytes.length - 8, 0x49454e44);
  return bytes;
}
function jpeg(width: number, height: number): Uint8Array<ArrayBuffer> {
  return new Uint8Array([255,216,255,192,0,11,8,height>>8,height&255,width>>8,width&255,1,1,17,0,
    255,218,0,8,1,1,0,0,63,0,12,255,0,22,255,217]);
}
describe("signature import allocation boundary", () => {
  it("reads bounded PNG and JPEG dimensions and permits the exact pixel ceiling", () => {
    expect(signatureDimensions(png(4096,2048))).toEqual({width:4096,height:2048});
    expect(signatureDimensions(jpeg(40,20))).toEqual({width:40,height:20});
  });
  it("refuses bombs, animation, truncation, duplicate frames and late dimension changes before decode", async () => {
    const late = jpeg(40,20); const dnl = new Uint8Array([...late.slice(0,-2),255,220,0,4,255,255,255,217]);
    const duplicate = new Uint8Array([...late.slice(0,-2),...late.slice(2,15),255,217]);
    const decode = vi.fn();
    for (const bytes of [png(4096,2049),png(8193,1),png(0,1),png(1,1,true),png(1,1).slice(0,-1),
      jpeg(65535,65535),jpeg(40,0),late.slice(0,-2),dnl,duplicate,new Uint8Array(10*1024*1024+1)]) {
      await expect(decodeSignature(new Blob([bytes]), decode)).rejects.toThrow();
    }
    expect(decode).not.toHaveBeenCalled();
  });
  it("closes a decoder result that disagrees with its header", async () => {
    const close = vi.fn();
    const bitmap = {width:41,height:20,close} as unknown as ImageBitmap;
    await expect(decodeSignature(new Blob([jpeg(40,20)]), async () => bitmap)).rejects.toThrow("disagree");
    expect(close).toHaveBeenCalledOnce();
    const correct = {...bitmap,width:40} as ImageBitmap;
    expect(await decodeSignature(new Blob([jpeg(40,20)]), async () => correct)).toBe(correct);
    const oriented = {...bitmap,width:20,height:40} as ImageBitmap;
    expect(await decodeSignature(new Blob([jpeg(40,20)]), async () => oriented)).toBe(oriented);
    expect(close).toHaveBeenCalledOnce();
  });
});


import { migrateSignature, type SignatureStorage } from "./signaturestore";
describe("protected signature migration", () => {
  it("removes plaintext only after saving and reading back identical protected pixels", async () => {
    let raw: string | null = JSON.stringify(image), saved: SignatureImage | null = null;
    const actions: string[] = [];
    const store: SignatureStorage = async (action) => {
      actions.push(action.kind); if (action.kind === "save") saved = structuredClone(action.image); return saved;
    };
    await migrateSignature({getItem:()=>raw, removeItem:()=>{actions.push("remove");raw=null;}}, store);
    expect(saved).toEqual(image); expect(raw).toBeNull();
    expect(actions).toEqual(["load","save","load","remove"]);
  });
  it("keeps plaintext on inaccessible, conflicting, corrupt or concurrently changed storage", async () => {
    for (const mode of ["failure","conflict","readback","changed","invalid"]) {
      let raw = mode === "invalid" ? "{}" : JSON.stringify(image), reads = 0;
      const removeItem = vi.fn();
      const store: SignatureStorage = async (action) => {
        if (mode === "failure") throw new Error("locked");
        if (action.kind === "load") {
          reads++;
          if (mode === "conflict") return {...image,width:1,rgba:image.rgba.slice(0,4)};
          if (reads > 1) {
            if (mode === "changed") raw = "changed";
            return mode === "readback" ? null : image;
          }
        }
        return null;
      };
      await expect(migrateSignature({getItem:()=>raw,removeItem},store)).rejects.toThrow(mode === "conflict" ? "old and protected saved signatures differ" : undefined);
      expect(removeItem).not.toHaveBeenCalled();
    }
  });
  it("does not contact protected storage when no legacy image exists", async () => {
    const store = vi.fn();
    await migrateSignature({getItem:()=>null,removeItem:vi.fn()},store);
    expect(store).not.toHaveBeenCalled();
  });
});
