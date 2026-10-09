import { describe, expect, it } from "vitest";
import type { Commands } from "./ipc";
import { FURTHER_VIEWS, NO_VIEW, ViewIds, isFurther, viewOf } from "./views";

describe("the id of a view", () => {
  it("is the document's handle for its first view", () => {
    expect(viewOf(7)).toBe(7);
    expect(isFurther(viewOf(7))).toBe(false);
    expect(isFurther(NO_VIEW)).toBe(false);
  });

  it("is a number no handle has for a further view, and a new one each time", () => {
    const ids = new ViewIds();
    const first = ids.another();
    const second = ids.another();
    expect(first).toBe(FURTHER_VIEWS);
    expect(second).not.toBe(first);
    expect(isFurther(first) && isFurther(second)).toBe(true);
    // A handle is a 32-bit count in the backend.
    expect(FURTHER_VIEWS).toBeGreaterThan(2 ** 32);
    expect(Number.isSafeInteger(FURTHER_VIEWS + 1_000_000)).toBe(true);
  });

  it("is not taken where a backend command wants a handle", () => {
    // What `call("close_document", ...)` is checked against.
    const sent = (args: Commands["close_document"]["args"]): number => args.doc;
    const handle: number = 4;
    expect(sent({ doc: handle })).toBe(4);
    // The first view of a document equals its handle, so this would work until
    // a document was shown twice. `npm run check` is what holds the line below:
    // without the brand on `DocHandle` the directive is unused, which is an error.
    // @ts-expect-error a view names a tab, and the backend knows documents
    expect(sent({ doc: viewOf(4) })).toBe(4);
  });
});
