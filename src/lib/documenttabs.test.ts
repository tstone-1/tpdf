import { describe, expect, it } from "vitest";
import { DocumentTabs, DocumentTasks } from "./documenttabs";
import { handleWindowKey, registerAppCommands, type AppActions } from "./appcommands";
import { CommandRegistry } from "./commands";
import { matches } from "./keys";

const tab = (id: number, path = `/fixture/${id}.pdf`) => ({ doc: { id }, path, edits: [id], page: id });

describe("document tabs", () => {
  it("keeps independent edit journals and positions across switches", () => {
    const tabs = new DocumentTabs<ReturnType<typeof tab>>();
    tabs.keep(tab(1));
    tabs.keep(tab(2));
    tabs.find(1)!.edits.push(10);
    tabs.find(1)!.page = 7;
    tabs.active = 1;
    expect(tabs.neighbour(1)?.doc.id).toBe(2);
    expect(tabs.neighbour(-1)?.doc.id).toBe(2);
    expect(tabs.find(1)).toMatchObject({ page: 7, edits: [1, 10] });
    expect(tabs.find(2)).toMatchObject({ page: 2, edits: [2] });
  });

  it("reuses Windows path spellings but distinguishes POSIX case", () => {
    const tabs = new DocumentTabs<ReturnType<typeof tab>>();
    tabs.keep(tab(1, "C:\\fixture\\Example.pdf"));
    tabs.keep(tab(2, "/fixture/Example.pdf"));
    expect(tabs.forPath("c:/fixture/example.pdf")?.doc.id).toBe(1);
    expect(tabs.forPath("/fixture/example.pdf")).toBeUndefined();
    expect(tabs.forPath("/fixture/Example.pdf")?.doc.id).toBe(2);
  });

  it("replaces a saved handle in its original position", () => {
    const tabs = new DocumentTabs<ReturnType<typeof tab>>();
    tabs.keep(tab(1)); tabs.keep(tab(2)); tabs.keep(tab(3));
    tabs.keep(tab(4, "/fixture/2.pdf"), 2);
    expect(tabs.all.map((entry) => entry.doc.id)).toEqual([1, 4, 3]);
    expect(tabs.find(2)).toBeUndefined();
    expect(tabs.active).toBe(4);
  });

  it("closes a background tab without moving focus and chooses a neighbour for the active tab", () => {
    const tabs = new DocumentTabs<ReturnType<typeof tab>>();
    tabs.keep(tab(1)); tabs.keep(tab(2)); tabs.keep(tab(3));
    tabs.active = 3;
    expect(tabs.remove(1)?.doc.id).toBe(1);
    expect(tabs.active).toBe(3);
    tabs.active = 2;
    tabs.remove(2);
    expect(tabs.active).toBe(3);
    tabs.remove(3);
    expect(tabs.active).toBe(-1);
    expect(tabs.neighbour(1)).toBeUndefined();
    expect(tabs.remove(999)).toBeUndefined();
  });

  it("wraps navigation and chooses the left tab when the last tab closes", () => {
    const tabs = new DocumentTabs<ReturnType<typeof tab>>();
    tabs.keep(tab(1)); tabs.keep(tab(2)); tabs.keep(tab(3));
    expect(tabs.neighbour(1)?.doc.id).toBe(1);
    tabs.active = 1;
    expect(tabs.neighbour(-1)?.doc.id).toBe(3);
    tabs.active = 3;
    tabs.remove(3);
    expect(tabs.active).toBe(2);
  });
});

describe("document tasks", () => {
  it("holds a transition until a save and its reopen finish", async () => {
    let finish!: () => void;
    const events: string[] = [];
    const tasks = new DocumentTasks();
    const save = tasks.run(async () => {
      events.push("save");
      await new Promise<void>((resolve) => { finish = resolve; });
      events.push("reopen");
    });
    const switchTab = tasks.idle().then(() => events.push("switch"));
    await Promise.resolve();
    expect(events).toEqual(["save"]);
    expect(tasks.busy).toBe(true);
    finish();
    await Promise.all([save, switchTab]);
    expect(events).toEqual(["save", "reopen", "switch"]);
    expect(tasks.busy).toBe(false);
  });

  it("releases waiters after failure and refuses an overlapping write", async () => {
    const states: boolean[] = [];
    const tasks = new DocumentTasks((busy) => states.push(busy));
    const first = tasks.run(async () => { throw new Error("fixture refusal"); });
    let second = false;
    const overlap = tasks.run(async () => { second = true; });
    await expect(first).rejects.toThrow("fixture refusal");
    await expect(overlap).rejects.toThrow("fixture refusal");
    await tasks.idle();
    expect(second).toBe(false);
    expect(states).toEqual([true, false]);
    await tasks.run(async () => { second = true; });
    expect(second).toBe(true);
  });
});

describe("tab commands", () => {
  it("routes shortcuts and palette commands to the tab actions", () => {
    const calls: string[] = [];
    const actions = {
      viewer: () => ({}), busyOpening: () => false, documentCount: () => 2,
      closeDocument: () => calls.push("close"),
      nextDocument: (delta: number) => calls.push(String(delta)),
    } as unknown as AppActions;
    const registry = new CommandRegistry();
    registerAppCommands(registry, actions);
    registry.run("file.close"); registry.run("view.nextTab"); registry.run("view.previousTab");
    const key = (key: string, shiftKey = false) => ({
      key, ctrlKey: true, metaKey: false, altKey: false, shiftKey,
      preventDefault() {},
    }) as KeyboardEvent;
    const deps = { actions, palette: () => null, hasDocument: () => true, refreshRecents() {} };
    handleWindowKey(key("w"), deps);
    handleWindowKey(key("Tab"), deps);
    handleWindowKey(key("Tab", true), deps);
    expect(calls).toEqual(["close", "1", "-1", "close", "1", "-1"]);
    actions.busyOpening = () => true;
    handleWindowKey(key("w"), deps);
    expect(calls).toHaveLength(6);
    expect(matches("view.nextTab", { ...key("Tab"), ctrlKey: false, metaKey: true } as KeyboardEvent)).toBe(false);
  });
});
