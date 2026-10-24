/** Temporary recognised text belongs to one viewer, never to the saved PDF.
 * Search and selection must read the same characters. A mixed search run is
 * answered one page at a time so its carry and indices stay in that source.
 */
import { call, type Commands } from "./ipc";
import type { PageText } from "./text";

export class SearchRecovery {
  private pages = new Map<string, PageText>();
  private key(doc: number, page: number): string { return `${doc}:${page}`; }

  put(doc: number, page: number, text: PageText): void {
    const total = [...this.pages.entries()].reduce((n, [key, value]) => n +
      (key === this.key(doc, page) ? 0 : value.codes.length), text.codes.length);
    if (total > 1_000_000) throw new Error("Recognition memory is full. Reopen the document to start again.");
    this.pages.set(this.key(doc, page), text);
  }

  clear(): void { this.pages.clear(); }
  get size(): number { return this.pages.size; }
  has(doc: number, page: number): boolean { return this.pages.has(this.key(doc, page)); }

  text = (args: Commands["page_text"]["args"]): Promise<PageText> => {
    const text = this.pages.get(this.key(args.doc, args.page));
    return text ? Promise.resolve(text) : call("page_text", args);
  };

  search = (args: Commands["search_page"]["args"]): Promise<Commands["search_page"]["reply"]> => {
    const pages = args.pages?.length ? args.pages : [args.page];
    if (!pages.some(page => this.pages.has(this.key(args.doc, page)))) {
      return call("search_page", args);
    }
    const page = pages[0]!;
    const text = this.pages.get(this.key(args.doc, page));
    return text
      ? call("search_text", { page, codes: text.codes, query: args.query, options: args.options, carry: args.carry })
      : call("search_page", { ...args, page, pages: [page] });
  };
}
