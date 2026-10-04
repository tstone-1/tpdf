import { describe, expect, it } from "vitest";

import type { Form, FormWidget } from "./forms";
import { pageId, type PageView } from "./pages";
import { emptySignatureFields, isEmptySignature, signTarget } from "./signfield";

function widget(n: number, name: string, extra: Partial<FormWidget> = {}): FormWidget {
  return {
    object: [n, 0], widget: [n, 0], page: 0, rect: [0, 0, 0, 0], display_rect: [20, 30, 180, 70], name,
    value: "", control: { kind: "signature", signed: false }, multiline: false, max_length: null,
    reason: "This field type is not supported yet",
    ...extra,
  };
}

const PAGES: PageView[] = [
  { id: pageId(8), source: { baseline: 1 }, turns: 0 },
  { id: pageId(5), source: { baseline: 0 }, turns: 0 },
];

describe("the signature fields somebody can sign", () => {
  const form: Form = {
    widgets: [
      widget(11, "Name", { control: { kind: "text" }, reason: null }),
      widget(12, "Signed", { control: { kind: "signature", signed: true } }),
      widget(13, "Approved"),
      widget(14, "Twice"),
      widget(15, "Twice", { page: 1 }),
      widget(16, "Witness", { page: 1 }),
      widget(17, "Other", { control: { kind: "unsupported" } }),
    ],
  };

  it("are the ones that hold no signature, in the form's order, each in one place", () => {
    expect(emptySignatureFields(form).map((one) => one.name)).toEqual(["Approved", "Witness"]);
    expect(emptySignatureFields(null)).toEqual([]);
    expect(form.widgets.map(isEmptySignature)).toEqual([false, false, true, true, true, true, false]);
  });

  it("are signed where they are: by name, on the page's id, in the rectangle as displayed", () => {
    expect(signTarget(form.widgets[2]!, PAGES)).toEqual({ name: "Approved", page: 5, rect: [20, 30, 180, 70] });
    expect(signTarget(form.widgets[5]!, PAGES)).toEqual({ name: "Witness", page: 8, rect: [20, 30, 180, 70] });
    // A copy, so that nothing the signing does to it reaches the form.
    expect(signTarget(form.widgets[2]!, PAGES)?.rect).not.toBe(form.widgets[2]!.display_rect);
  });

  it("are not offered on a page the file does not show, nor when signed or of another kind", () => {
    expect(signTarget(form.widgets[5]!, [PAGES[1]!])).toBeNull();
    expect(signTarget(form.widgets[1]!, PAGES)).toBeNull();
    expect(signTarget(form.widgets[0]!, PAGES)).toBeNull();
  });
});
