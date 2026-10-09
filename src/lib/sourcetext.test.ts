import { describe, expect, it } from "vitest";

import app from "../App.svelte?raw";
import { functionIn, missingFrom, withoutComments } from "./sourcetext";

describe("source with its comments removed", () => {
  it("drops a line that is only in a comment, of each kind", () => {
    const source = [
      "const sent = send({ run: 0 }); // send({ run: runs.number })",
      "/* guard(); */",
      "/**",
      " * check();",
      " */",
      "<!-- <button onclick={stop}> -->",
      "keep();",
    ].join("\n");
    const code = withoutComments(source);
    expect(code).toContain("const sent = send({ run: 0 });");
    expect(code).toContain("keep();");
    for (const gone of ["runs.number", "guard();", "check();", "onclick={stop}"]) {
      expect(code, gone).not.toContain(gone);
    }
    expect(missingFrom(source, ["send({ run: runs.number })", "keep();"])).toEqual([
      "send({ run: runs.number })",
    ]);
  });

  it("keeps what only looks like a comment", () => {
    const source = [
      'const at = "https://example.com/a"; const b = \'//not\';',
      "const c = `x // ${ y /* gone */ } // z`;",
      "const d = /^https?:\\/\\//.test(at) ? a / b : c;",
      "const e = `${ `inner // kept` }`;",
    ].join("\n");
    const code = withoutComments(source);
    expect(code).toContain('"https://example.com/a"');
    expect(code).toContain("'//not'");
    expect(code).toContain("`x // ${ y  } // z`");
    expect(code).toContain("/^https?:\\/\\//.test(at) ? a / b : c;");
    expect(code).toContain("`${ `inner // kept` }`");
  });

  it("does not see whether a line runs, which is the limit of every check built on it", () => {
    // Written down as a test so that nobody takes a presence check for more:
    // each of these is found, and none of them does anything.
    const dead = [
      "if (false) { guard(); }",
      "function unused() { other(); }",
      "return; after();",
    ].join("\n");
    expect(missingFrom(dead, ["guard();", "other();", "after();"])).toEqual([]);
  });

  it("compares a line with its white space collapsed", () => {
    const source = "call(\n    one,\n    two,\n  );";
    expect(missingFrom(source, ["call( one, two, );"])).toEqual([]);
    expect(missingFrom(source, ["call(one, two)"])).toEqual(["call(one, two)"]);
  });

  it("reads the whole of the component without losing its end", () => {
    // The control on the scanner itself: a string or a template it failed to
    // close would swallow the rest of the file, and every line after it would
    // be reported missing for the wrong reason.
    const code = withoutComments(app);
    expect(code).toContain("</script>");
    expect(code.trimEnd().endsWith("</style>")).toBe(true);
    expect(code.length).toBeLessThan(app.length);
    expect(code).not.toContain("/**");
  });
});

describe("one function of a source", () => {
  const source = [
    "  function first(): void {",
    "    one();",
    "    // two();",
    "  }",
    "",
    "  function second(): void {",
    "    two();",
    "  }",
  ].join("\n");

  it("ends where the function does, and has no comments", () => {
    const first = functionIn(source, "function first(): void {");
    expect(first).toContain("one();");
    expect(first).not.toContain("two();");
  });

  it("refuses a beginning that is not there, or is there twice", () => {
    expect(() => functionIn(source, "function third(")).toThrow("not in the source");
    expect(() => functionIn(source, "(): void {")).toThrow("in the source twice");
  });
});
