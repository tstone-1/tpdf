import { expect, it, vi } from "vitest";

import { runAutobenchIfRequested } from "./autobench";

const { invoke, fetchTile } = vi.hoisted(() => ({
  invoke: vi.fn(),
  fetchTile: vi.fn(),
}));
vi.mock("./ipc", () => ({ call: invoke, isOpenRefusal: () => false }));
vi.mock("./tiles", () => ({ fetchRequiredTile: fetchTile }));

it("counts server time once and computes the residual for each request before taking the median", async () => {
  let output = "";
  invoke.mockImplementation((command: string, args?: { text: string }) => {
    if (command === "autobench_path") return "bench.pdf";
    if (command === "open_document") {
      return { id: 1, page_count: 1, pages: [{ width_pt: 612, height_pt: 792 }] };
    }
    if (command === "spike_print" && args) output = args.text;
  });
  let requests = 0;
  fetchTile.mockImplementation(() => {
    // Four interleaved variants per round. Each sample takes 50 ms in all.
    const round = Math.floor(requests++ / 4);
    const render = [10, 20, 30][round % 3]!;
    const encode = [20, 20, 0][round % 3]!;
    const decode = [2, 18, 10][round % 3]!;
    return {
      bitmap: { close() {} },
      renderUs: render * 1000,
      encodeUs: encode * 1000,
      transferMs: 50 - decode,
      decodeMs: decode,
      bytes: 1024,
    };
  });

  await runAutobenchIfRequested();
  const row = output.split("\n").find(line => line.startsWith("png  1024"));
  expect(row?.trim().split(/\s+/).slice(2)).toEqual([
    "20.00", "20.00", "40.00", "10.00", "50.00", "20.00", "1", "67%",
  ]);
  expect(invoke).toHaveBeenCalledWith("spike_exit", { code: 0 });
});
