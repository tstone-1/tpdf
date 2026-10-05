import type { CommandRegistry } from "./commands";
import { PALETTE, type Swatch } from "./markcolors";
import { NIBS } from "./marknibs";
import { message } from "./i18n";
import type { IconName } from "./icons";
import type { MarkKind } from "./pages";

export interface ToolItem {
  id: string;
  label: string;
  swatch?: Swatch;
  /** Drawn beside the label in the tool row. */
  icon?: IconName;
}

export interface ToolGroup {
  /** Stable identity: translated labels must never decide behavior. */
  id: "document" | "highlight" | "draw" | "form" | "pages" | "redact" | "color" | "width";
  label: string;
  /** Drawn beside the label on the group's own button. */
  icon?: IconName;
  items: ToolItem[];
}

/** Presentation only: command execution and guards stay in the registry. */
export const TOOL_ACTIONS: ToolItem[] = [
  { id: "edit.addComment", label: "Comment", icon: "comment" },
  { id: "edit.addTextBox", label: "Text box", icon: "textBox" },
  { id: "edit.editText", label: "Edit text", icon: "editText" },
  { id: "edit.addSignature", label: "Signature image", icon: "signature" },
];

export const TOOL_GROUPS: ToolGroup[] = [
  {
    id: "document",
    label: "Document",
    icon: "document",
    items: [
      { id: "file.properties", label: "Document properties" },
      { id: "file.saveCopy", label: "Save a copy..." },
      { id: "file.print", label: "Print..." },
      { id: "file.reload", label: "Reload from disk" },
    ],
  },
  {
    id: "highlight",
    label: "Highlight",
    icon: "highlight",
    items: [
      { id: "edit.highlightSelection", label: "Highlight selection" },
      { id: "edit.underlineSelection", label: "Underline selection" },
      { id: "edit.strikeoutSelection", label: "Strike out selection" },
      { id: "edit.squigglySelection", label: "Squiggly underline selection" },
    ],
  },
  {
    id: "draw",
    label: "Draw",
    icon: "draw",
    items: [
      { id: "edit.draw", label: "Freehand drawing" },
      { id: "edit.drawBox", label: "Rectangle" },
      { id: "edit.drawEllipse", label: "Ellipse" },
      { id: "edit.stamp.approved", label: "Approved stamp" },
      { id: "edit.stamp.confidential", label: "Confidential stamp" },
      { id: "edit.stamp.draft", label: "Draft stamp" },
      { id: "edit.stamp.final", label: "Final stamp" },
      { id: "edit.erase", label: "Erase marks" },
    ],
  },
  {
    // The menu bar's Form menu, in its order. Both border commands are listed
    // because only one of the two is enabled at a time, and the other is greyed.
    id: "form",
    label: "Form",
    icon: "form",
    items: [
      { id: "edit.fillForm", label: "Fill form" },
      { id: "edit.addTextField", label: "Add a text field" },
      { id: "edit.addMultilineField", label: "Add a text field on several lines" },
      { id: "edit.addCheckbox", label: "Add a checkbox" },
      { id: "edit.addDropdown", label: "Add a dropdown..." },
      { id: "edit.addRadio", label: "Add radio buttons..." },
      { id: "edit.addSignatureField", label: "Add a signature field" },
      { id: "edit.fieldBorderOn", label: "Draw a line round new text fields" },
      { id: "edit.fieldBorderOff", label: "No line round new text fields" },
      { id: "edit.formEditOn", label: "Change the document's own fields" },
      { id: "edit.formEditOff", label: "Finish changing the document's fields" },
      { id: "edit.fieldProperties", label: "Properties of the picked field..." },
      { id: "edit.tabOrder", label: "Tab through fields in reading order" },
    ],
  },
  {
    id: "pages",
    label: "Pages",
    icon: "pages",
    items: [
      { id: "view.showThumbnails", label: "Page thumbnails" },
      { id: "edit.rotatePageClockwise", label: "Rotate page clockwise" },
      { id: "edit.rotatePageCounterClockwise", label: "Rotate page counterclockwise" },
      { id: "edit.cropToDrag", label: "Crop by dragging" },
      { id: "edit.cropToContent", label: "Crop to content" },
      { id: "edit.resetCrop", label: "Reset crop" },
      { id: "edit.insertBlankPage", label: "Insert blank page" },
      { id: "edit.insertPages", label: "Insert pages from file..." },
      { id: "edit.movePageUp", label: "Move page earlier" },
      { id: "edit.movePageDown", label: "Move page later" },
      { id: "file.extractPages", label: "Extract pages..." },
      { id: "file.splitDocument", label: "Split document..." },
      { id: "file.mergeDocuments", label: "Merge documents..." },
      { id: "edit.deletePage", label: "Delete page" },
    ],
  },
  {
    id: "redact",
    label: "Redact",
    icon: "redact",
    items: [
      { id: "edit.redactRegion", label: "Mark a region for removal" },
      { id: "edit.redactSelection", label: "Mark selected text for removal" },
      { id: "edit.redactMatches", label: "Mark search matches for removal" },
      { id: "view.showRedactions", label: "Review marked regions" },
      { id: "file.redactRasterCopy", label: "Redact to image-only copy..." },
      { id: "file.redactCopy", label: "Redact and save as..." },
      { id: "file.redactDocument", label: "Redact and save" },
    ],
  },
  {
    id: "color",
    label: message("color"),
    items: PALETTE.map((entry) => ({
      id: `edit.color.${entry.id}`,
      label: entry.name[0]!.toUpperCase() + entry.name.slice(1),
      swatch: entry,
    })),
  },
  {
    id: "width",
    label: "Width",
    icon: "width",
    items: NIBS.map((entry) => ({
      id: `edit.nib.${entry.id}`,
      label: `${entry.name[0]!.toUpperCase()}${entry.name.slice(1)} (${entry.pt} pt)`,
    })),
  },
];

const HEADER_COMMANDS = [
  "file.save", "file.saveCopy", "file.print", "edit.undo", "edit.redo",
  "nav.previousPage", "nav.nextPage", "nav.goToPage", "view.zoomIn",
  "view.zoomOut", "view.fitWidth", "view.fitPage", "view.actualSize", "view.zoomTo",
];

/** Refresh from the shell's status/edit notifications; guards are not reactive. */
export function toolbarState(
  registry: CommandRegistry,
): Record<string, { enabled: boolean; title: string }> {
  const ids = [
    ...HEADER_COMMANDS,
    ...TOOL_ACTIONS.map((item) => item.id),
    ...TOOL_GROUPS.flatMap((group) => group.items.map((item) => item.id)),
  ];
  return Object.fromEntries(ids.map((id) => {
    const command = registry.find(id);
    return [id, {
      enabled: command !== undefined && (command.enabled?.() ?? true),
      title: command
        ? `${command.title}${command.keys ? ` (${command.keys})` : ""}`
        : "Unavailable",
    }];
  }));
}

/** What the viewer reports as armed; `ViewerStatus.armed`'s type. */
export type ArmedTool = MarkKind | "crop" | "redact" | "place" | null;

/** The kinds a chosen colour changes. A stamp and a signature keep their own. */
const COLORED: readonly ArmedTool[] = ["note", "square", "ellipse", "textbox", "ink"];

/** The kinds drawn as a line, which is what a width is the width of. */
const STROKED: readonly ArmedTool[] = ["square", "ellipse", "ink"];

/**
 * Which of the colour and width controls the armed tool's status row carries.
 *
 * They used to sit at the end of the tool row always, and took a row of their
 * own in a window of the default width, for two settings that only matter to
 * the next mark. Now each is offered exactly while the tool that is armed would
 * use it, in the row that names that tool --- a row that is already there, so
 * arming a tool moves the page once and choosing a colour never does.
 *
 * A highlight is not an armed tool: it is applied to a selection in one click.
 * Its colour is chosen in the Highlight menu, beside the four kinds, and that
 * is deliberately not this function's business --- a control that appeared on
 * the tool row whenever text was selected would shift the toolbar under a
 * reader who is in the middle of dragging out a selection.
 *
 * `drawing` is asked separately from `armed` because the viewer reports an armed
 * pen through `drawing` alone --- see `ViewerStatus.armed`.
 */
export function styleOptions(tool: {
  armed: ArmedTool;
  drawing: boolean;
  erasing: boolean;
}): { color: boolean; width: boolean } {
  if (tool.erasing) return { color: false, width: false };
  return {
    color: tool.drawing || COLORED.includes(tool.armed),
    width: tool.drawing || STROKED.includes(tool.armed),
  };
}

/** The least room a menu keeps between itself and the window's edge, in pixels. */
export const POPUP_MARGIN = 8;

/**
 * How far to move a menu sideways so that all of it is inside the window.
 *
 * A menu opens under its button, aligned to one of its edges, and that is
 * decided in the stylesheet without knowing where the button is. At 1,000 px
 * the More button wraps to the start of a second row, and its menu, aligned to
 * the button's right edge, opened to the left of the window with only its last
 * letters showing; the Redact menu at the end of the row can run off the right
 * the same way. So the menu is measured once it is open and moved by this much.
 *
 * `left` and `right` are the menu's edges and `viewport` the window's width.
 * Positive moves right. A menu wider than the window is put at the left margin,
 * because the start of each line is the part that says which item it is.
 */
export function popupShift(left: number, right: number, viewport: number): number {
  if (left < POPUP_MARGIN || right - left > viewport - 2 * POPUP_MARGIN) {
    return POPUP_MARGIN - left;
  }
  const limit = viewport - POPUP_MARGIN;
  return right > limit ? limit - right : 0;
}
