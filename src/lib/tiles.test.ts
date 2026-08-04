import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

/**
 * Which platform shape the mocked Tauri is standing in for.
 *
 * Both shapes are Tauri's own. The Windows one is the whole reason this file
 * exists: WebView2 cannot register a URI scheme, so a hardcoded
 * `tile://localhost` fetches nothing there --- and the viewer still boots, lays
 * out the document and runs its frame loop, so the symptom is a blank page
 * rather than an error. A test that only ever exercised the macOS shape would
 * pass just as well with the bug present, so every assertion below is made
 * against both.
 */
let shape: "scheme" | "http" = "scheme";

const convertFileSrc = vi.fn((path: string, protocol: string) =>
  shape === "http"
    ? `http://${protocol}.localhost/${encodeURIComponent(path)}`
    : `${protocol}://localhost/${encodeURIComponent(path)}`,
);

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string, protocol: string) =>
    convertFileSrc(path, protocol),
}));

const { DocumentGone, fetchTile, resetTileOrigin, tileOrigin, tileUrl } =
  await import("./tiles");

beforeEach(() => {
  resetTileOrigin();
  convertFileSrc.mockClear();
});

afterEach(() => {
  resetTileOrigin();
});

describe("the tile origin", () => {
  it("is the custom scheme where the webview registers one", () => {
    shape = "scheme";
    expect(tileOrigin()).toBe("tile://localhost/");
  });

  it("is an http origin where the webview cannot", () => {
    shape = "http";
    expect(tileOrigin()).toBe("http://tile.localhost/");
  });

  it("is asked for once and remembered", () => {
    shape = "scheme";
    tileOrigin();
    tileOrigin();
    expect(convertFileSrc).toHaveBeenCalledTimes(1);
  });
});

describe("a tile url", () => {
  const request = {
    doc: 3,
    page: 7,
    scale: 1.5,
    x: 10,
    y: 20,
    width: 512,
    height: 512,
    format: "raw" as const,
  };

  it("is built on whatever origin this platform serves", () => {
    shape = "scheme";
    expect(tileUrl(request)).toBe(
      "tile://localhost/3/7/1500/10/20/512/512?fmt=raw",
    );
    resetTileOrigin();
    shape = "http";
    expect(tileUrl(request)).toBe(
      "http://tile.localhost/3/7/1500/10/20/512/512?fmt=raw",
    );
  });

  it("keeps the path separators the server parses, on both shapes", () => {
    // The reason `convertFileSrc` supplies the origin only. Handing it the whole
    // path percent-encodes the separators, and the server splits on them --- so
    // an encoded URL is refused rather than mis-parsed, but it is refused on
    // every single tile.
    for (const next of ["scheme", "http"] as const) {
      resetTileOrigin();
      shape = next;
      const url = tileUrl(request);
      expect(url).not.toContain("%2F");
      expect(url.split("?")[0]).toContain("/3/7/1500/10/20/512/512");
    }
  });
});

describe("a tile response the server refused", () => {
  const request = {
    rid: 1,
    doc: 3,
    page: 7,
    scale: 1.5,
    turns: 0 as const,
    invert: false,
    x: 10,
    y: 20,
    width: 512,
    height: 512,
    format: "raw" as const,
  };

  /** A response with just enough of the interface for `fetchTile` to read. */
  function reply(status: number, body: string): Response {
    return {
      ok: status >= 200 && status < 300,
      status,
      text: () => Promise.resolve(body),
    } as unknown as Response;
  }

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("becomes a DocumentGone when the document's file is gone", async () => {
    // The half of the chain `scroller.test.ts` cannot see: it mocks this module
    // wholesale, so it proves what the scroller does with a `DocumentGone` and
    // says nothing about what produces one. Together they are the path from a
    // truncated file to a reader being told.
    vi.stubGlobal("fetch", () =>
      Promise.resolve(reply(410, "this file changed on disk")),
    );
    await expect(fetchTile(request)).rejects.toBeInstanceOf(DocumentGone);
    await expect(fetchTile(request)).rejects.toThrow(
      "this file changed on disk",
    );
  });

  it("stays an ordinary Error for every other refusal", async () => {
    // The control, and it is the one that matters: `DocumentGone` stops the
    // scroller asking for good, so a 400 promoted into one would silently
    // freeze a document over a single malformed request. `toBeInstanceOf` alone
    // cannot catch that --- every DocumentGone is an Error too.
    vi.stubGlobal("fetch", () => Promise.resolve(reply(400, "page 3 of 2")));
    const failure = await fetchTile(request).catch((e: unknown) => e);
    expect(failure).toBeInstanceOf(Error);
    expect(failure).not.toBeInstanceOf(DocumentGone);
  });
});
