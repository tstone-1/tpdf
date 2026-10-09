// Types of `download.js`, for the test in `src/lib/sitedownload.test.ts`.

export interface Installers {
  mac: string | null;
  windows: string | null;
  version: string | null;
}

export const LATEST: string;
export function installers(release: unknown): Installers;
