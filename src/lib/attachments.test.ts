import { describe, expect, it } from "vitest";

import { attachmentRows, describe as describeFile, isProgram, kindOf, pick, preview, saved } from "./attachments";

const files = [
  { name: "orders.csv", description: "the order lines", size: 1200 },
  { name: "invoice.xml", description: "", size: null },
  { name: "Orders-2.CSV", description: "", size: 10 },
];

describe("what an attachment's name claims", () => {
  it("names the kind by the ending, whatever its case", () => {
    expect(kindOf("report.XLSX")).toBe("an Excel workbook");
    expect(kindOf("notes.txt")).toBe("a text file");
    expect(kindOf("data.parquet")).toBe("a .parquet file");
    expect(kindOf("README")).toBe("a file with no ending to say what it is");
    expect(kindOf(".profile")).toBe("a file with no ending to say what it is");
  });

  it("calls a name that ends like a program one, and warns when it is saved", () => {
    expect(kindOf("setup.exe")).toBe("a program or script");
    expect(isProgram("run.SH")).toBe(true);
    expect(isProgram("invoice.pdf")).toBe(false);
    expect(saved({ name: "setup.exe", description: "", size: null }, 2048, "setup.exe"))
      .toContain("open it only if you trust the document");
    expect(saved(files[1]!, 12, "invoice.xml")).toBe("Saved invoice.xml to invoice.xml, 12 bytes.");
  });

  it("describes a file with its stated size, and leaves out a size it does not state", () => {
    expect(describeFile(files[0]!)).toBe("orders.csv (a table of comma-separated values, 1.2 KB (1,200 bytes))");
    expect(describeFile(files[1]!)).toBe("invoice.xml (an XML file)");
  });
});

describe("which attachment an answer names", () => {
  it("takes a number from 1, and refuses one past the list", () => {
    expect(pick("2", files)).toEqual({ index: 1 });
    expect(pick(" 3 ", files)).toEqual({ index: 2 });
    expect(pick("0", files)).toEqual({ problem: "There is no attachment 0; there are 3" });
    expect(pick("4", files)).toEqual({ problem: "There is no attachment 4; there are 3" });
  });

  it("takes part of exactly one name, ignoring case", () => {
    expect(pick("XML", files)).toEqual({ index: 1 });
    expect(pick("orders", files)).toEqual({ problem: '2 attachments have "orders" in their names; type a number' });
    expect(pick("pdf", files)).toEqual({ problem: 'No attachment\'s name has "pdf" in it' });
    expect(pick("", files)).toEqual({ problem: "Which attachment: 1 to 3, or part of its name" });
  });

  it("previews only an answer that names one", () => {
    expect(preview("1", files)).toBe("Saves orders.csv (a table of comma-separated values, 1.2 KB (1,200 bytes))");
    expect(preview("orders", files)).toBe("");
  });
});

describe("the properties dialog's rows", () => {
  it("numbers the files as the palette does, with a description where there is one", () => {
    expect(attachmentRows(files.slice(0, 2))).toEqual([
      { name: "Attachment 1", value: "orders.csv (a table of comma-separated values, 1.2 KB (1,200 bytes)): the order lines" },
      { name: "Attachment 2", value: "invoice.xml (an XML file)" },
    ]);
  });
});
