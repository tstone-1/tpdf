import { describe, expect, it } from "vitest";
import { DEFAULT_TAB_LABEL_SIZE, TAB_LABEL_SIZES, TabLabelSize } from "./tablabels";

it("offers the sizes the guards stop at", () => {
  expect(TAB_LABEL_SIZES[0]).toBe(9);
  expect(TAB_LABEL_SIZES[TAB_LABEL_SIZES.length - 1]).toBe(16);
});

function memory(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  return {
    values,
    store: () => ({
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => { values.set(key, value); },
    }),
  };
}

describe("tab label size", () => {
  it("starts at the default and steps through the offered sizes, stopping at both ends", () => {
    const { store, values } = memory();
    const size = new TabLabelSize(store);
    expect(size.px).toBe(DEFAULT_TAB_LABEL_SIZE);
    expect(size.isDefault).toBe(true);
    expect(size.step(1)).toBe(12);
    expect(values.get("tpdf.tabLabelSize")).toBe("12");
    for (let i = 0; i < 10; i++) size.step(1);
    expect(size.px).toBe(16);
    expect(size.canGrow).toBe(false);
    for (let i = 0; i < 10; i++) size.step(-1);
    expect(size.px).toBe(9);
    expect(size.canShrink).toBe(false);
    expect(size.step(0)).toBe(DEFAULT_TAB_LABEL_SIZE);
  });

  it("reads back a saved size and ignores one it does not offer", () => {
    expect(new TabLabelSize(memory({ "tpdf.tabLabelSize": "14" }).store).px).toBe(14);
    for (const saved of ["15", "abc", "", "1e3"]) {
      expect(new TabLabelSize(memory({ "tpdf.tabLabelSize": saved }).store).px)
        .toBe(DEFAULT_TAB_LABEL_SIZE);
    }
  });

  it("keeps working when storage throws", () => {
    const broken = () => ({
      getItem: () => { throw new Error("denied"); },
      setItem: () => { throw new Error("denied"); },
    });
    const size = new TabLabelSize(broken);
    expect(size.px).toBe(DEFAULT_TAB_LABEL_SIZE);
    expect(size.step(-1)).toBe(10);
    expect(size.px).toBe(10);
  });
});
