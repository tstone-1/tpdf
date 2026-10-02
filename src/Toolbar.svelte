<script lang="ts">
  import { tick } from "svelte";
  import { TOOL_ACTIONS, TOOL_GROUPS, popupShift, styleOptions, type ArmedTool, type ToolGroup, type ToolItem } from "./lib/toolbar";
  import { icon } from "./lib/icons";
  import { DEFAULT_SWATCH, swatch, swatchBackground } from "./lib/markcolors";
  import { message } from "./lib/i18n";

  let {
    state: commandState,
    active = null,
    armed = null,
    drawing = null,
    erasing = false,
    selected = 0,
    colorId = "default",
    widthLabel = "",
    run,
    finish,
    cancel,
  }: {
    state: Record<string, { enabled: boolean; title: string }>;
    active: string | null;
    armed?: ArmedTool;
    drawing: number | null;
    erasing: boolean;
    selected: number;
    colorId?: string;
    widthLabel?: string;
    run: (id: string) => void;
    finish: () => void;
    cancel: () => void;
  } = $props();

  let host = $state<HTMLDivElement>();
  let open = $state<string | null>(null);
  const basicGroups = TOOL_GROUPS.filter((group) =>
    ["highlight", "draw"].includes(group.id),
  );
  const otherGroups = TOOL_GROUPS.filter((group) =>
    ["pages", "redact"].includes(group.id),
  );
  const options = TOOL_GROUPS.filter((group) =>
    ["color", "width"].includes(group.id),
  );
  const colors = TOOL_GROUPS.find((group) => group.id === "color")!;
  let selectedSwatch = $derived(swatch(colorId) ?? DEFAULT_SWATCH);
  let unfinished = $derived((drawing ?? 0) > 0);
  let inTool = $derived(active !== null || drawing !== null || erasing);
  let offered = $derived(styleOptions({ armed, drawing: drawing !== null, erasing }));

  function toolPressed(id: string): boolean {
    if (id === "draw") return drawing !== null || erasing || /^(Box|Ellipse|Stamp)/.test(active ?? "");
    if (id === "edit.addComment") return active?.startsWith("Comment") ?? false;
    if (id === "edit.addTextBox") return active?.startsWith("Text box") ?? false;
    if (id === "pages") return active?.startsWith("Crop") ?? false;
    if (id === "redact") return active?.startsWith("Redact") ?? false;
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
    void commandState; void drawing; void active; void erasing; void open; void host; void offered;
    void tick().then(() => rove());
  });

  $effect(() => {
    // An open menu is measured and moved inside the window; see `popupShift`.
    // The transform is cleared first, so the measurement is of where the
    // stylesheet put the menu and not of where the last correction left it.
    void open; void host;
    void tick().then(() => {
      const popup = host?.querySelector<HTMLElement>(".popup");
      if (!popup) return;
      popup.style.transform = "";
      const box = popup.getBoundingClientRect();
      const shift = popupShift(box.left, box.right, window.innerWidth);
      if (shift !== 0) popup.style.transform = `translateX(${shift}px)`;
    });
  });

  function enabled(id: string): boolean {
    return (commandState[id]?.enabled ?? false) && !unfinished;
  }

  function invoke(id: string) {
    open = null;
    run(id);
  }

  /**
   * A colour is a setting, not an action: the menu it was picked from stays open.
   *
   * `run` hands the keyboard to the page before it dispatches, and focus leaving
   * the toolbar closes whatever menu is open. So the menu is put back in the
   * same turn, before anything is redrawn, and the swatch takes the focus again.
   */
  function choose(id: string) {
    const menu = open;
    run(id);
    open = menu;
    void tick().then(() => host?.querySelector<HTMLButtonElement>(`button[data-choice="${id}"]`)?.focus());
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

  /** The colour and width buttons show a picture only, so this is their name. */
  function optionLabel(group: ToolGroup): string {
    return `${group.label}: ${group.id === "color" ? selectedSwatch.name : widthLabel}`;
  }

  function toggle(label: string) {
    open = open === label ? null : label;
  }
</script>

<svelte:window onpointerdown={outside} onkeydowncapture={keyboard} onfocusin={focusMoved} onresize={() => { open = null; rove(); }} />

{#snippet itemButton(item: ToolItem, option = false)}
  <button
    class:color-item={item.swatch !== undefined}
    aria-pressed={item.swatch ? item.swatch.id === selectedSwatch.id : undefined}
    disabled={!(commandState[item.id]?.enabled ?? false) || (unfinished && !option)}
    title={item.swatch?.id === "default" ? message("defaultColorHint") : commandState[item.id]?.title ?? item.label}
    onclick={() => invoke(item.id)}
  >
    {#if item.swatch}
      <span class="swatch" style:background={swatchBackground(item.swatch)} aria-hidden="true"></span>
    {/if}
    {item.label}
    {#if item.swatch}
      <span class="selection" class:selected={item.swatch.id === selectedSwatch.id} aria-hidden="true"></span>
    {/if}
  </button>
{/snippet}

{#snippet dropdown(group: ToolGroup, option = false)}
  <div class="dropdown">
    <button
      class:pressed={open === group.id || toolPressed(group.id)}
      aria-pressed={["draw", "pages", "redact"].includes(group.id) ? toolPressed(group.id) : undefined}
      data-group={group.id}
      data-testid={group.id === "color" ? "markcolor" : group.id === "width" ? "marknib" : undefined}
      aria-expanded={open === group.id}
      disabled={unfinished && !option}
      title={group.id === "highlight" && selected === 0 ? "Select text first, then choose a highlight or underline" : option ? optionLabel(group) : group.label}
      aria-label={option ? optionLabel(group) : undefined}
      onclick={() => toggle(group.id)}
    >{#if group.id === "color"}<span class="swatch" style:background={swatchBackground(selectedSwatch)} aria-hidden="true"></span>{:else if group.icon}<span class="icon" use:icon={group.icon}></span>{/if}{#if !option}{group.label}{/if}<span class="chevron" aria-hidden="true"></span></button>
    {#if open === group.id}
      <div class="popup" aria-label={group.label}>
        {#if group.id === "highlight" && selected === 0}
          <p class="hint">Select text first</p>
        {/if}
        {#each group.items as item}
          {@render itemButton(item, option)}
        {/each}
        {#if group.id === "highlight"}
          <p class="hint">{colors.label}: {selectedSwatch.name}</p>
          <div class="swatches" role="group" aria-label={colors.label}>
            {#each colors.items as item}
              <button class="swatch-button" data-choice={item.id} aria-pressed={item.swatch!.id === selectedSwatch.id}
                aria-label={item.label} title={item.swatch!.id === "default" ? message("defaultColorHint") : item.label}
                onclick={() => choose(item.id)}>
                <span class="swatch" style:background={swatchBackground(item.swatch!)} aria-hidden="true"></span>
              </button>
            {/each}
          </div>
        {/if}
      </div>
    {/if}
  </div>
{/snippet}

<div class="tools" bind:this={host} role="toolbar" aria-label="Document tools">
  <!-- Keep Document visible at narrow widths too: Windows has no native File menu. -->
  {#each TOOL_GROUPS.filter((group) => group.id === "document") as group}
    {@render dropdown(group)}
  {/each}
  <div class="history">
    <button disabled={!enabled("edit.undo")} title={commandState["edit.undo"]?.title ?? "Undo"} aria-label="Undo" onclick={() => invoke("edit.undo")}><span class="icon" use:icon={"undo"}></span></button>
    <button disabled={!enabled("edit.redo")} title={commandState["edit.redo"]?.title ?? "Redo"} aria-label="Redo" onclick={() => invoke("edit.redo")}><span class="icon" use:icon={"redo"}></span></button>
  </div>
  <button class:pressed={!inTool} aria-pressed={!inTool} disabled={unfinished} title={unfinished ? "Finish or discard this drawing first" : "Select text and marks"} onclick={cancel}><span class="icon" use:icon={"select"}></span>Select</button>
  {#each basicGroups.filter((group) => group.id === "highlight") as group}
    {@render dropdown(group)}
  {/each}
  {#each TOOL_ACTIONS as action}
    <button class:pressed={toolPressed(action.id)} aria-pressed={toolPressed(action.id)} disabled={!enabled(action.id)} title={commandState[action.id]?.title ?? action.label} onclick={() => invoke(action.id)}>{#if action.icon}<span class="icon" use:icon={action.icon}></span>{/if}{action.label}</button>
  {/each}
  {#each basicGroups.filter((group) => group.id === "draw") as group}
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
          <p class="hint">{group.label}: {group.id === "color" ? selectedSwatch.name : widthLabel}</p>
          {#each group.items as item}
            {@render itemButton(item, true)}
          {/each}
        {/each}
      </div>
    {/if}
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
      <!-- What the armed tool draws with, in the row that names it; see `styleOptions`. -->
      {#if offered.color || offered.width}
        <div class="options">
          {#each options.filter((group) => group.id === "color" ? offered.color : offered.width) as group}{@render dropdown(group, true)}{/each}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .tools { display: flex; align-items: center; gap: 3px; min-height: 40px; padding: 3px 10px; box-sizing: border-box; flex-wrap: wrap; flex: none; border-bottom: 1px solid color-mix(in srgb, CanvasText 15%, transparent); background: Canvas; color: CanvasText; font: 13px/1.4 system-ui, sans-serif; -webkit-user-select: none; user-select: none; }
  button { font: inherit; color: inherit; background: transparent; border: 1px solid transparent; border-radius: 4px; min-height: 32px; padding: 4px 9px; white-space: nowrap; cursor: pointer; display: inline-flex; align-items: center; gap: 6px; }
  button:hover:not(:disabled), .pressed { background: color-mix(in srgb, CanvasText 9%, Canvas); }
  button:focus-visible { outline: 2px solid Highlight; outline-offset: 1px; }
  button:disabled { opacity: .45; cursor: default; }
  .pressed { border-color: color-mix(in srgb, CanvasText 25%, Canvas); }
  .history, .secondary, .options { display: flex; align-items: center; gap: 3px; }
  .history { border-right: 1px solid color-mix(in srgb, CanvasText 18%, transparent); padding-right: 6px; margin-right: 3px; }
  .options { margin-left: auto; }
  .dropdown { position: relative; }
  .chevron { display: inline-block; width: 5px; height: 5px; border-bottom: 1px solid; border-right: 1px solid; transform: rotate(45deg); margin: 0 2px 3px 2px; }
  .popup { position: absolute; top: calc(100% + 4px); left: 0; z-index: 50; display: flex; flex-direction: column; min-width: 175px; max-width: min(310px, calc(100vw - 24px)); max-height: min(65vh, 440px); overflow-y: auto; padding: 5px; border: 1px solid color-mix(in srgb, CanvasText 25%, Canvas); background: Canvas; border-radius: 6px; box-shadow: 0 4px 16px #0003; }
  .popup button { text-align: left; white-space: normal; }
  .swatch { display: inline-block; width: 16px; height: 16px; flex: none; box-sizing: border-box; border-radius: 50%; border: 1px solid color-mix(in srgb, CanvasText 45%, transparent); vertical-align: -3px; }
  .popup button { display: block; }
  .popup .color-item { display: flex; align-items: center; gap: 8px; }
  .color-item[aria-pressed="true"] { background: color-mix(in srgb, CanvasText 9%, Canvas); }
  .selection { width: 8px; height: 8px; flex: none; margin-left: auto; border-radius: 50%; }
  .selection.selected { background: CanvasText; }
  .options .popup, .more { left: auto; right: 0; }
  .hint { font-size: 12px; margin: 5px 9px; opacity: .7; }
  .swatches { display: flex; gap: 2px; padding: 0 4px 3px; }
  .popup .swatch-button { display: inline-flex; min-height: 28px; padding: 4px; }
  .swatch-button[aria-pressed="true"] { border-color: CanvasText; }
  .compact { display: none; }
  .tool-state { display: flex; align-items: center; gap: 6px; width: 100%; min-height: 32px; border-top: 1px solid color-mix(in srgb, CanvasText 10%, Canvas); }
  .tool-state span { min-width: 0; overflow-wrap: anywhere; }
  .tool-state > button, .tool-state .dropdown > button { flex-shrink: 0; border-color: color-mix(in srgb, CanvasText 20%, Canvas); }
  .finish { color: HighlightText; background: Highlight; }
  @media (max-width: 1180px) { .secondary { display: none; } .compact { display: block; } }
  @media (max-width: 680px) { .tools { gap: 1px; padding-inline: 5px; } button { padding-inline: 6px; } .options { margin-left: 0; } .popup { position: fixed; top: auto; left: 8px; right: 8px; min-width: 0; max-width: none; } .options .popup, .more { left: 8px; right: 8px; } }
</style>
