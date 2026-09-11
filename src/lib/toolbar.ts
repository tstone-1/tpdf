import type { CommandRegistry } from "./commands";
import { PALETTE } from "./markcolors";
import { NIBS } from "./marknibs";

export interface ToolItem {
  id: string;
  label: string;
}

export interface ToolGroup {
  label: string;
  items: ToolItem[];
}

/** Presentation only: command execution and guards stay in the registry. */
export const TOOL_ACTIONS: ToolItem[] = [
  { id: "edit.addComment", label: "Comment" },
  { id: "edit.addTextBox", label: "Text" },
  { id: "edit.addSignature", label: "Sign" },
];

export const TOOL_GROUPS: ToolGroup[] = [
  {
    label: "Document",
    items: [
      { id: "file.properties", label: "Document properties" },
      { id: "file.saveCopy", label: "Save a copy..." },
      { id: "file.print", label: "Print..." },
      { id: "file.reload", label: "Reload from disk" },
    ],
  },
  {
    label: "Highlight",
    items: [
      { id: "edit.highlightSelection", label: "Highlight selection" },
      { id: "edit.underlineSelection", label: "Underline selection" },
      { id: "edit.strikeoutSelection", label: "Strike out selection" },
      { id: "edit.squigglySelection", label: "Squiggly underline selection" },
    ],
  },
  {
    label: "Draw",
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
    label: "Pages",
    items: [
      { id: "view.showThumbnails", label: "Page thumbnails" },
      { id: "edit.rotatePageClockwise", label: "Rotate page clockwise" },
      { id: "edit.rotatePageCounterClockwise", label: "Rotate page counterclockwise" },
      { id: "edit.cropToDrag", label: "Crop by dragging" },
      { id: "edit.cropToContent", label: "Crop to content" },
      { id: "edit.resetCrop", label: "Reset crop" },
      { id: "edit.insertBlankPage", label: "Insert blank page" },
      { id: "edit.movePageUp", label: "Move page earlier" },
      { id: "edit.movePageDown", label: "Move page later" },
      { id: "file.extractPages", label: "Extract pages..." },
      { id: "file.splitDocument", label: "Split document..." },
      { id: "file.mergeDocuments", label: "Merge documents..." },
      { id: "edit.deletePage", label: "Delete page" },
    ],
  },
  {
    label: "Redact",
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
    label: "Colour",
    items: PALETTE.map((entry) => ({
      id: `edit.color.${entry.id}`,
      label: entry.name[0]!.toUpperCase() + entry.name.slice(1),
    })),
  },
  {
    label: "Width",
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
