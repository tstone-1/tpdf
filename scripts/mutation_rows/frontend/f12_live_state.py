"""Frontend mutation rows: what one document's walks keep (`CoveredWords`,
`CommentWords`, `RegionWords`), held as `LiveDocument` fields rather than as
variables of `App.svelte`, and the two halves of the disk-change window phase
that a unit test can reach.

Added 2026-11-19 with the move; each was first proven red by hand.
"""

from __future__ import annotations

from .base import Mutation

ROWS = [
    # --- coveredwords.ts --------------------------------------------------------------
    Mutation("covered words: kept whole", "src/lib/coveredwords.ts",
        "    this.words.set(id, text.slice(0, COVERED_CHARS));",
        "    this.words.set(id, text);",
        "are cut to the bound, so a hundred highlighted pages are not held whole"),
    Mutation("covered words: a tab keeps the live list itself", "src/lib/coveredwords.ts",
        "    const kept = new CoveredWords();\n    for (const [id, words] of this.words) kept.words.set(id, words);\n    return kept;",
        "    return this;",
        "are kept by a tab as a copy the live one does not reach"),
    Mutation("covered words: restored on top of another document's", "src/lib/coveredwords.ts",
        "    this.words.clear();\n    for (const [id, words] of kept.words)",
        "    for (const [id, words] of kept.words)",
        "are restored in place, so a view sharing the list sees them"),
    Mutation("covered words: restored from themselves clears them", "src/lib/coveredwords.ts",
        "    if (kept === this) return;\n",
        "",
        "restored from themselves are left as they are"),
    # --- commentwalk.ts: CommentWords ---------------------------------------------------
    Mutation("comment words: a second walk runs beside the first", "src/lib/commentwalk.ts",
        "  async fill(walk: Omit<CommentWalk, \"asked\">): Promise<void> {\n    if (this.filling) return;",
        "  async fill(walk: Omit<CommentWalk, \"asked\">): Promise<void> {",
        "stands down while a comment walk is running"),
    Mutation("comment words: the flag stays up after a failure", "src/lib/commentwalk.ts",
        "          return true;\n        },\n      });\n    } finally {\n      this.filling = false;\n    }",
        "          return true;\n        },\n      });\n      this.filling = false;\n    } finally {\n    }",
        "lowers the comment walk's flag when it fails"),
    Mutation("comment words: a fresh set each walk", "src/lib/commentwalk.ts",
        "        asked: this.asked,",
        "        asked: new Set<number>(),",
        "asks no comment twice across calls"),
    Mutation("comment words: words kept for a document that has gone", "src/lib/commentwalk.ts",
        "          if (!walk.found(words)) return false;\n          for (const [comment, said] of words) this.words.set(comment, said);",
        "          for (const [comment, said] of words) this.words.set(comment, said);\n          if (!walk.found(words)) return false;",
        "keeps no words for a document that has gone"),
    # --- redactlist.ts: RegionWords -----------------------------------------------------
    Mutation("region words: a second walk runs beside the first", "src/lib/redactlist.ts",
        "  async fill(walk: Omit<RedactionWalk, \"words\" | \"planned\">): Promise<void> {\n    if (this.filling) return;",
        "  async fill(walk: Omit<RedactionWalk, \"words\" | \"planned\">): Promise<void> {",
        "stands down while a region walk is running"),
    Mutation("region words: the flag stays up after a failure", "src/lib/redactlist.ts",
        "      await fillRedactionRegions({ ...walk, words: this.words, planned: this.plans });\n    } finally {\n      this.filling = false;\n    }",
        "      await fillRedactionRegions({ ...walk, words: this.words, planned: this.plans });\n      this.filling = false;\n    } finally {\n    }",
        "lowers the region walk's flag when it fails"),
    Mutation("region words: the plans not kept between walks", "src/lib/redactlist.ts",
        "words: this.words, planned: this.plans });",
        "words: this.words, planned: new Map() });",
        "keeps each region's words and plan in its own maps"),
    # --- App.svelte: the walks hold their document's state ---------------------------
    Mutation("region walk: the window hands over a fresh state", "src/App.svelte",
        "const id = openView, reader = viewer, panel = sidebar, walk = redactionWords;",
        "const id = openView, reader = viewer, panel = sidebar, walk = new RegionWords();",
        "is held by each walk for its length"),
    # --- diskchangecheck.ts -----------------------------------------------------------
    Mutation("disk change: the hold shorter than the watch needs", "src/lib/diskchangecheck.ts",
        "  return Math.max(controlMs, 3 * WATCH_INTERVAL_MS) + 2 * WATCH_INTERVAL_MS;",
        "  return controlMs + 2 * WATCH_INTERVAL_MS;",
        "outlasts the three checks the watch needs"),
    Mutation("disk change: the hold no longer than the control", "src/lib/diskchangecheck.ts",
        "  return Math.max(controlMs, 3 * WATCH_INTERVAL_MS) + 2 * WATCH_INTERVAL_MS;",
        "  return Math.max(controlMs, 5 * WATCH_INTERVAL_MS);",
        "outlasts what the control took on this machine"),
]
