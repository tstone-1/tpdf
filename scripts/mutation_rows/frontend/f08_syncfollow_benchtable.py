"""Frontend mutation rows for the scroll-lock follower (`syncfollow.ts`) and the
benchmark tables' column helper (`benchtable.ts`).

`syncfollow.ts` holds what a frame of one locked document does to the other.
Its three pieces of state are the ones a defect hides in: the flag that tells
the partner's own frames from the reader's, the fitted zoom the window sets,
and the spell after a wheel turned with Alt.
"""

from __future__ import annotations

from .base import Mutation

ROWS = [
    # --- syncfollow.ts -------------------------------------------------------------
    Mutation("sync follow: the partner's own frames answered as the reader's", "src/lib/syncfollow.ts",
        "    this.#following = true;\n    try {",
        "    this.#following = false;\n    try {",
        "does not send a move back to the side it came from"),
    Mutation("sync follow: a zoom the window made passed across", "src/lib/syncfollow.ts",
        "{ following, fitted });",
        "{ following, fitted: false });",
        "passes a zoom the reader set across, and not one the window made"),
    Mutation("sync follow: a wheel with Alt starts no spell alone", "src/lib/syncfollow.ts",
        "    this.#aloneUntil = this.now() + ALONE_MS;",
        "    this.#aloneUntil = 0;",
        "scrolls one side alone for a moment after a wheel with Alt"),
    Mutation("sync follow: alone at every moment but the spell", "src/lib/syncfollow.ts",
        "alone: this.now() < this.#aloneUntil,",
        "alone: this.now() > this.#aloneUntil,",
        "scrolls one side alone for a moment after a wheel with Alt"),
    # --- benchtable.ts -------------------------------------------------------------
    Mutation("bench table: a label filled on the left and a figure on the right", "src/lib/benchtable.ts",
        "  return right ? text.padStart(width) : text.padEnd(width);",
        "  return right ? text.padEnd(width) : text.padStart(width);",
        "puts a label on the left and a figure on the right of its column"),
]
