"""Frontend mutation rows: drags that stay on their page, a page order that
changes under a gesture, every reply limit worded, answers the mirror does not
know, codes no character has, the screen-reader writer that stops, and AltGr.

Added 2026-11-19 with the fixes they guard; each was first proven red by hand.
"""

from __future__ import annotations

from .base import Mutation

ROWS = [
    # --- viewer.ts: a drag stays on the page it started on --------------------------
    Mutation("viewer: a region drag follows the pointer onto the next page", "src/lib/viewer.ts",
        "        live.to = this.pointOn(live.slot, at);\n        this.wake();",
        "        live.to = this.pageAndPoint(at);\n        this.wake();",
        "keeps a region that ends inside the next page at the first page's bottom"),
    Mutation("viewer: the eraser measures in the page under the pointer", "src/lib/viewer.ts",
        "          const { x, y } = this.pointOn(this.doomed.slot, at);",
        "          const { x, y } = this.pageAndPoint(at);",
        "takes nothing at the top of the first page when the sweep crosses into the next"),
    Mutation("viewer: ink points leave the page", "src/lib/viewer.ts",
        "        const laid = keepOnPage(this.pointOn(live.slot, at), this.laidSize(live.slot));",
        "        const laid = this.pointOn(live.slot, at);",
        "keeps every point of an ink stroke on its page"),
    Mutation("viewer: another kind inherits the strokes in progress", "src/lib/viewer.ts",
        "    if (this.drawArmed !== kind) this.inking = null;\n",
        "",
        "starts from no corners when armed after two ink strokes"),
    # --- viewer.ts: the page order changes under a gesture --------------------------
    Mutation("viewer: a new page order keeps Back and Forward", "src/lib/viewer.ts",
        "    this.moveDrag.cancel();\n"
        "    // Saved jumps name slots in the previous page order.\n"
        "    this.history.clear();\n",
        "    this.moveDrag.cancel();\n",
        "forgets Back and Forward, which name slots the edit renumbered"),
    Mutation("viewer: a crop answered after close is adopted", "src/lib/viewer.ts",
        "      if (this.pages !== pages || this.life.ended) return;\n      if (!at) continue;",
        "      if (this.pages !== pages) return;\n      if (!at) continue;",
        "is dropped, and redraws nothing"),
    Mutation("viewer: a cancelled selection drag keeps following", "src/lib/viewer.ts",
        "    this.root.addEventListener(\"pointercancel\", this.onSelectCancel);\n",
        "",
        "marks nothing, follows the pointer no further, and leaves no listener"),
    Mutation("viewer: a cancelled scrollbar drag keeps following", "src/lib/viewer.ts",
        "    this.track.addEventListener(\"pointercancel\", this.onTrackPointerUp);\n",
        "",
        "lets a cancelled scrollbar drag go, so the thumb stops following"),
    # --- every reply limit is worded -------------------------------------------------
    Mutation("properties: an unread timestamp token goes unsaid", "src/lib/properties.ts",
        "  say(said.timestampsName, limits.timestamps_unread, said.timestampsUnread);\n",
        "",
        "each key, switched on alone, changes what the reader is told"),
    Mutation("outline: clipped titles go unsaid", "src/lib/sidebar.ts",
        "  if (limits.titles_clipped) parts.push(text.titlesClipped(limits.titles_clipped));\n",
        "",
        "each key, switched on alone, changes what the reader is told"),
    # --- integrity.ts: an answer this version does not know --------------------------
    Mutation("trust: an unknown standing is worded as a known one", "src/lib/integrity.ts",
        "  if (!isKnown(STANDINGS, trust.standing)) return unknownRow(said.trustName, trust.standing);\n",
        "",
        "is said to be unknown, as a warning, for every row worded from an enum"),
    Mutation("integrity: an unknown reason is looked up anyway", "src/lib/integrity.ts",
        "  return isKnown(known, value) ? word(table[value]) : said.unknownReason(value);",
        "  return word(table[value as K]);",
        "is a reason this version does not know, never a throw, inside a known answer"),
    Mutation("properties: an unknown level loses its warning on the face", "src/lib/properties.ts",
        "    out.push(level?.warn ? { text: signature.pades, warn: true } : { text: signature.pades });",
        "    out.push({ text: signature.pades });",
        "is the same unknown sentence on a closed card, with the warning"),
    # --- text.ts: a code no character has ------------------------------------------
    Mutation("text: a code past U+10FFFF reaches fromCodePoint", "src/lib/text.ts",
        "    out += chunk.every((code) => Number.isInteger(code) && code >= 0 && code <= 0x10ffff)",
        "    out += true",
        "reads as the replacement character, in a copy and in a double-click"),
    # --- livetext.ts and a11y.ts: a writer that stops ------------------------------
    Mutation("livetext: stop leaves the focus listener", "src/lib/livetext.ts",
        "      pending = null;\n      unlisten();\n",
        "      pending = null;\n",
        "takes its focus listener away and writes nothing it was still holding"),
    Mutation("a11y: destroy leaves the writer listening", "src/lib/a11y.ts",
        "    this.stopHearing();\n",
        "",
        "is what destroying the screen-reader layer does"),
    # --- keys.ts: AltGr --------------------------------------------------------------
    Mutation("keys: AltGr fires an accel+Alt binding", "src/lib/keys.ts",
        "  if (binding.alt && event.ctrlKey && !event.metaKey && event.getModifierState?.(\"AltGraph\") === true) return false;\n",
        "",
        "refuses an accel+Alt chord when the browser says the key was AltGr"),
    # --- commands.ts: positions in a title whose lower case is longer ----------------
    Mutation("commands: positions from the lower-cased title", "src/lib/commands.ts",
        "  const hay = foldCase(title);",
        "  const hay = title.toLowerCase();",
        "answers positions in the title as written, where lower case would lengthen it"),
]
