import { describe, expect, test } from "vitest";
import { installers, platformOf } from "../../site/download.js";

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

  test("pass over an entry that is not a file with a name", () => {
    const odd = {
      tag_name: "v26.10.13",
      assets: [
        null,
        { browser_download_url: `${BASE}tpdf_26.10.13_aarch64.dmg` },
        { name: 7, browser_download_url: `${BASE}tpdf_26.10.13_x64-setup.exe` },
        ...NAMES.map((name) => asset(name)),
      ],
    };
    expect(installers(odd)).toEqual({
      mac: `${BASE}tpdf_26.10.13_aarch64.dmg`,
      windows: `${BASE}tpdf_26.10.13_x64-setup.exe`,
      version: "26.10.13",
    });
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

describe("the landing page's own-platform button", () => {
  // Real user agents, as each browser sends them. Android's says `Linux` too,
  // which is the case the exclusion exists for: there is no build for it.
  const AGENTS = {
    windows:
      "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36",
    mac: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15",
    linux: "Mozilla/5.0 (X11; Linux x86_64; rv:143.0) Gecko/20100101 Firefox/143.0",
    android:
      "Mozilla/5.0 (Linux; Android 15; Pixel 9) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Mobile Safari/537.36",
  };

  test("marks Windows, Mac and Linux each as their own", () => {
    expect(platformOf(AGENTS.windows)).toBe("windows");
    expect(platformOf(AGENTS.mac)).toBe("mac");
    expect(platformOf(AGENTS.linux)).toBe("linux");
  });

  test("marks nothing for Android, which says Linux and has no build", () => {
    // Mutation: the `!/Android/` test removed, which survived every test
    // until this one (review of 2026-10-15, N3).
    expect(platformOf(AGENTS.android)).toBeNull();
    expect(platformOf("")).toBeNull();
  });
});
