import { describe, expect, test } from "vitest";
import { installers } from "../../site/download.js";

// The file names of release 26.10.13, as GitHub's API lists them.
const BASE = "https://github.com/tstone-1/tpdf/releases/download/v26.10.13/";
const NAMES = [
  "latest.json",
  "tpdf_26.10.13_aarch64.dmg",
  "tpdf_26.10.13_x64-setup.exe",
  "tpdf_26.10.13_x64-setup.exe.sig",
  "tpdf_aarch64.app.tar.gz",
  "tpdf_aarch64.app.tar.gz.sig",
];
const asset = (name: string, base = BASE) => ({ name, browser_download_url: base + name });
const release = (names: string[]) => ({ tag_name: "v26.10.13", assets: names.map((name) => asset(name)) });

describe("the landing page's download links", () => {
  test("link the two installers of a release and name its version", () => {
    expect(installers(release(NAMES))).toEqual({
      mac: `${BASE}tpdf_26.10.13_aarch64.dmg`,
      windows: `${BASE}tpdf_26.10.13_x64-setup.exe`,
      version: "26.10.13",
    });
  });

  test("do not take the installer's signature file for the installer", () => {
    const withoutInstaller = NAMES.filter((name) => name !== "tpdf_26.10.13_x64-setup.exe");
    expect(installers(release(withoutInstaller)).windows).toBeNull();
  });

  test("link nothing for a platform with two installers", () => {
    const two = [...NAMES, "tpdf_26.10.14_aarch64.dmg"];
    const found = installers(release(two));
    expect(found.mac).toBeNull();
    expect(found.windows).toBe(`${BASE}tpdf_26.10.13_x64-setup.exe`);
  });

  test("link nothing that is not downloaded from this project's releases", () => {
    const elsewhere = {
      tag_name: "v26.10.13",
      assets: [asset("tpdf_26.10.13_aarch64.dmg", "https://example.com/")],
    };
    expect(installers(elsewhere).mac).toBeNull();
  });

  test("name no version for a tag that is not one", () => {
    expect(installers({ tag_name: "pdfium-8066-tpdf.1", assets: [] }).version).toBeNull();
    expect(installers({ tag_name: "v26.10.14-rc1", assets: [] }).version).toBeNull();
  });

  test("answer nothing for a reply that is not a release", () => {
    const nothing = { mac: null, windows: null, version: null };
    expect(installers(null)).toEqual(nothing);
    expect(installers({ message: "API rate limit exceeded" })).toEqual(nothing);
  });
});
