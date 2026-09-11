<script lang="ts">
  import { tick } from "svelte";
  import { TOOL_ACTIONS, TOOL_GROUPS } from "./lib/toolbar";

  let {
    state: commandState,
    active = null,
    drawing = null,
    erasing = false,
    selected = 0,
    colourLabel = "",
    widthLabel = "",
    run,
    finish,
    cancel,
  }: {
    state: Record<string, { enabled: boolean; title: string }>;
    active: string | null;
    drawing: number | null;
    erasing: boolean;
    selected: number;
    colourLabel?: string;
    widthLabel?: string;
    run: (id: string) => void;
    finish: () => void;
    cancel: () => void;
  } = $props();

  let host = $state<HTMLDivElement>();
  let open = $state<string | null>(null);
  const basicGroups = TOOL_GROUPS.filter((group) =>
    ["Highlight", "Draw"].includes(group.label),
  );
  const otherGroups = TOOL_GROUPS.filter((group) =>
    ["Pages", "Redact"].includes(group.label),
  );
  const options = TOOL_GROUPS.filter((group) =>
    ["Colour", "Width"].includes(group.label),
  );
  let unfinished = $derived((drawing ?? 0) > 0);
  let inTool = $derived(active !== null || drawing !== null || erasing);

  function toolPressed(label: string): boolean {
    if (label === "Draw") return drawing !== null || erasing || /^(Box|Ellipse|Stamp)/.test(active ?? "");
    if (label === "Comment") return active?.startsWith("Comment") ?? false;
    if (label === "Text") return active?.startsWith("Text box") ?? false;
    if (label === "Pages") return active?.startsWith("Crop") ?? false;
    if (label === "Redact") return active?.startsWith("Redact") ?? false;
    return false;
  }

  function topButtons(): HTMLButtonElement[] {
    return Array.from(host?.querySelectorAll<HTMLButtonElement>("button") ?? [])
      .filter((button) => !button.closest(".popup") && !button.disabled && button.getClientRects().length > 0);
  }

  function rove(preferred?: HTMLButtonElement) {
    if (!host) return;
    const buttons = topButtons();
    const chosen = preferred && buttons.includes(preferred) ? preferred
      : buttons.find((button) => button === document.activeElement)
        ?? buttons.find((button) => button.tabIndex === 0)
        ?? buttons[0];
    for (const button of host.querySelectorAll<HTMLButtonElement>("button")) {
      if (!button.closest(".popup")) button.tabIndex = button === chosen ? 0 : -1;
    }
    const items = Array.from(host.querySelectorAll<HTMLButtonElement>(".popup button:not(:disabled)"));
    const item = items.find((button) => button === document.activeElement) ?? items[0];
    for (const button of host.querySelectorAll<HTMLButtonElement>(".popup button")) button.tabIndex = button === item ? 0 : -1;
  }

  $effect(() => {
    // Enablement and tool completion can remove the current toolbar tab stop.
    void commandState; void drawing; void active; void erasing; void open; void host;
    void tick().then(() => rove());
  });

  function enabled(id: string): boolean {
    return (commandState[id]?.enabled ?? false) && !unfinished;
  }

  function invoke(id: string) {
    open = null;
    run(id);
  }

  function outside(event: PointerEvent) {
    if (event.target instanceof Node && !host?.contains(event.target)) open = null;
  }

  function focusMoved(event: FocusEvent) {
    if (event.target instanceof Node && !host?.contains(event.target)) open = null;
    else if (event.target instanceof HTMLButtonElement) rove(event.target);
  }

  async function keyboard(event: KeyboardEvent) {
    if (!host) return;
    if (event.key === "Escape" && open !== null) {
      const label = open;
      open = null;
      event.preventDefault();
      event.stopPropagation();
      host.querySelector<HTMLButtonElement>(`button[data-group="${label}"]`)?.focus();
      return;
    }
    const target = event.target;
    if (!(target instanceof HTMLButtonElement) || !host.contains(target)) return;
    const popup = target.closest(".popup");
    const group = target.dataset.group;
    if (!popup && group && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
      event.preventDefault(); event.stopPropagation();
      open = group;
      await tick();
      const items = Array.from(host.querySelectorAll<HTMLButtonElement>(".popup button:not(:disabled)"));
      (event.key === "ArrowUp" ? items.at(-1) : items[0])?.focus();
      return;
    }
    const previous = popup ? "ArrowUp" : "ArrowLeft";
    const next = popup ? "ArrowDown" : "ArrowRight";
    if (![previous, next, "Home", "End"].includes(event.key)) return;
    event.preventDefault(); event.stopPropagation();
    const buttons = popup ? Array.from(popup.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")) : topButtons();
    const position = buttons.indexOf(target);
    const index = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1
      : (position + (event.key === previous ? -1 : 1) + buttons.length) % buttons.length;
    if (!popup) open = null;
    buttons[index]?.focus();
  }

  function toggle(label: string) {
    open = open === label ? null : label;
  }
</script>

<svelte:window onpointerdown={outside} onkeydowncapture={keyboard} onfocusin={focusMoved} onresize={() => { open = null; rove(); }} />

{#snippet dropdown(group: { label: string; items: { id: string; label: string }[] }, option = false)}
  <div class="dropdown">
    <button
      class:pressed={open === group.label || toolPressed(group.label)}
      aria-pressed={["Draw", "Pages", "Redact"].includes(group.label) ? toolPressed(group.label) : undefined}
      data-group={group.label}
      data-testid={group.label === "Colour" ? "markcolor" : group.label === "Width" ? "marknib" : undefined}
      aria-expanded={open === group.label}
      disabled={unfinished && !option}
      title={group.label === "Highlight" && selected === 0 ? "Select text first, then choose a highlight or underline" : group.label}
      onclick={() => toggle(group.label)}
    >{group.label}{group.label === "Colour" && colourLabel ? `: ${colourLabel}` : group.label === "Width" && widthLabel ? `: ${widthLabel}` : ""}<span class="chevron" aria-hidden="true"></span></button>
    {#if open === group.label}
      <div class="popup" aria-label={group.label}>
        {#if group.label === "Highlight" && selected === 0}
          <p class="hint">Select text first</p>
        {/if}
        {#each group.items as item}
          <button
            disabled={!(commandState[item.id]?.enabled ?? false) || (unfinished && !option)}
            title={commandState[item.id]?.title ?? item.label}
            onclick={() => invoke(item.id)}
          >{item.label}</button>
        {/each}
      </div>
    {/if}
  </div>
{/snippet}

<div class="tools" bind:this={host} role="toolbar" aria-label="Document tools">
  <!-- Keep Document visible at narrow widths too: Windows has no native File menu. -->
  {#each TOOL_GROUPS.filter((group) => group.label === "Document") as group}
    {@render dropdown(group)}
  {/each}
  <div class="history">
    <button disabled={!enabled("edit.undo")} title={commandState["edit.undo"]?.title ?? "Undo"} onclick={() => invoke("edit.undo")}>Undo</button>
    <button disabled={!enabled("edit.redo")} title={commandState["edit.redo"]?.title ?? "Redo"} onclick={() => invoke("edit.redo")}>Redo</button>
  </div>
  <button class:pressed={!inTool} aria-pressed={!inTool} disabled={unfinished} title={unfinished ? "Finish or discard this drawing first" : "Select text and marks"} onclick={cancel}>Select</button>
  {#each basicGroups.filter((group) => group.label === "Highlight") as group}
    {@render dropdown(group)}
  {/each}
  {#each TOOL_ACTIONS as action}
    <button class:pressed={toolPressed(action.label)} aria-pressed={toolPressed(action.label)} disabled={!enabled(action.id)} title={commandState[action.id]?.title ?? action.label} onclick={() => invoke(action.id)}>{action.label}</button>
  {/each}
  {#each basicGroups.filter((group) => group.label === "Draw") as group}
    {@render dropdown(group)}
  {/each}
  <div class="secondary">
    {#each otherGroups as group}{@render dropdown(group)}{/each}
  </div>
  <div class="compact dropdown">
    <button data-group="More tools" aria-expanded={open === "More tools"} onclick={() => toggle("More tools")}>More<span class="chevron" aria-hidden="true"></span></button>
    {#if open === "More tools"}
      <div class="popup more">
        {#each otherGroups as group}
          <p class="hint">{group.label}</p>
          {#each group.items as item}
            <button disabled={!enabled(item.id)} title={commandState[item.id]?.title ?? item.label} onclick={() => invoke(item.id)}>{item.label}</button>
          {/each}
        {/each}
        {#each options as group}
          <p class="hint">{group.label}: {group.label === "Colour" ? colourLabel : widthLabel}</p>
          {#each group.items as item}
            <button disabled={!(commandState[item.id]?.enabled ?? false)} title={commandState[item.id]?.title ?? item.label} onclick={() => invoke(item.id)}>{item.label}</button>
          {/each}
        {/each}
      </div>
    {/if}
  </div>
  <div class="options">
    {#each options as group}{@render dropdown(group, true)}{/each}
  </div>
  {#if inTool}
    <div class="tool-state" role="status">
      <span data-testid={drawing !== null ? "drawing" : erasing ? "erasing" : "armed"}>{drawing !== null ? `Drawing${drawing > 0 ? `: ${drawing} stroke${drawing === 1 ? "" : "s"}` : " - press and drag"}` : erasing ? "Erasing marks" : active}</span>
      {#if drawing !== null}
        <button class="finish" disabled={drawing === 0} onclick={finish}>Finish</button>
        <button onclick={cancel}>{drawing > 0 ? "Discard" : "Cancel"}</button>
      {:else}
        <button onclick={cancel}>{erasing ? "Stop" : "Cancel"}</button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .tools { display: flex; align-items: center; gap: 3px; min-height: 40px; padding: 3px 10px; box-sizing: border-box; flex-wrap: wrap; flex: none; border-bottom: 1px solid color-mix(in srgb, CanvasText 15%, transparent); background: Canvas; color: CanvasText; font: 13px/1.4 system-ui, sans-serif; -webkit-user-select: none; user-select: none; }
  button { font: inherit; color: inherit; background: transparent; border: 1px solid transparent; border-radius: 4px; min-height: 32px; padding: 4px 9px; white-space: nowrap; cursor: pointer; }
  button:hover:not(:disabled), .pressed { background: color-mix(in srgb, CanvasText 9%, Canvas); }
  button:focus-visible { outline: 2px solid Highlight; outline-offset: 1px; }
  button:disabled { opacity: .45; cursor: default; }
  .pressed { border-color: color-mix(in srgb, CanvasText 25%, Canvas); }
  .history, .secondary, .options { display: flex; align-items: center; gap: 3px; }
  .history { border-right: 1px solid color-mix(in srgb, CanvasText 18%, transparent); padding-right: 6px; margin-right: 3px; }
  .options { margin-left: auto; }
  .dropdown { position: relative; }
  .chevron { display: inline-block; width: 5px; height: 5px; border-bottom: 1px solid; border-right: 1px solid; transform: rotate(45deg); margin: 0 2px 3px 8px; }
  .popup { position: absolute; top: calc(100% + 4px); left: 0; z-index: 50; display: flex; flex-direction: column; min-width: 175px; max-width: min(310px, calc(100vw - 24px)); max-height: min(65vh, 440px); overflow-y: auto; padding: 5px; border: 1px solid color-mix(in srgb, CanvasText 25%, Canvas); background: Canvas; border-radius: 6px; box-shadow: 0 4px 16px #0003; }
  .popup button { text-align: left; white-space: normal; }
  .options .popup, .more { left: auto; right: 0; }
  .hint { font-size: 12px; margin: 5px 9px; opacity: .7; }
  .compact { display: none; }
  .tool-state { display: flex; align-items: center; gap: 6px; width: 100%; min-height: 32px; border-top: 1px solid color-mix(in srgb, CanvasText 10%, Canvas); }
  .tool-state span { min-width: 0; overflow-wrap: anywhere; }
  .tool-state button { flex-shrink: 0; border-color: color-mix(in srgb, CanvasText 20%, Canvas); }
  .finish { color: HighlightText; background: Highlight; }
  @media (max-width: 1100px) { .secondary, .options { display: none; } .compact { display: block; } }
  @media (max-width: 680px) { .tools { gap: 1px; padding-inline: 5px; } button { padding-inline: 6px; } .options { margin-left: 0; } .popup { position: fixed; top: auto; left: 8px; right: 8px; min-width: 0; max-width: none; } .options .popup, .more { left: 8px; right: 8px; } }
</style>
