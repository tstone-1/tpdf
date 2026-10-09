// Points the page's two download buttons at the installers of the latest
// release.
//
// WHY THE PAGE ASKS AT ALL. An installer's file name carries its version
// (`tpdf_26.10.13_aarch64.dmg`), so no fixed link reaches it, and the release
// page lists it among the updater's `.sig`, `.tar.gz` and `latest.json` files,
// which a reader does not need and has to tell apart. The page asks GitHub's
// public API which files the latest release has and links the two installers.
//
// IF THE ANSWER DOES NOT COME, or does not hold exactly one file for a
// platform, the button keeps the link it was written with: the release page.
// That is why `installers` answers `null` instead of guessing.
//
// To look at the page before it is published: copy `site/index.html`,
// `site/download.js` and `docs/img/` (as `img/`) into one folder and serve it,
// which is what `.github/workflows/pages.yml` does.

export const LATEST = "https://api.github.com/repos/tstone-1/tpdf/releases/latest";

// Where an installer of this project is downloaded from. A link that does not
// start here is not put on the page.
const DOWNLOADS = "https://github.com/tstone-1/tpdf/releases/download/";

// The end of each installer's name. Whole endings, so that
// `..._x64-setup.exe.sig` is not taken for the installer it signs.
const ENDING = { mac: "_aarch64.dmg", windows: "_x64-setup.exe" };

/** The one link among `assets` whose name ends with `ending`, or `null`. */
function only(assets, ending) {
  const links = assets
    .filter((asset) => typeof asset?.name === "string" && asset.name.endsWith(ending))
    .map((asset) => asset.browser_download_url)
    .filter((link) => typeof link === "string" && link.startsWith(DOWNLOADS));
  return links.length === 1 ? links[0] : null;
}

/**
 * The installers of one release, as GitHub's API describes it.
 * A platform is `null` when the release does not hold exactly one installer
 * for it; `version` is `null` when the tag is not `v` and a version.
 */
export function installers(release) {
  const assets = Array.isArray(release?.assets) ? release.assets : [];
  const tag = typeof release?.tag_name === "string" ? release.tag_name : "";
  return {
    mac: only(assets, ENDING.mac),
    windows: only(assets, ENDING.windows),
    version: /^v\d+\.\d+\.\d+$/.test(tag) ? tag.slice(1) : null,
  };
}

async function fill() {
  const reply = await fetch(LATEST, { headers: { Accept: "application/vnd.github+json" } });
  if (!reply.ok) return;
  const found = installers(await reply.json());
  for (const platform of ["mac", "windows"]) {
    const link = found[platform];
    const button = document.getElementById(`download-${platform}`);
    if (link && button) button.href = link;
  }
  const version = document.getElementById("version");
  if (found.version && version) version.textContent = `Version ${found.version}. `;
}

if (typeof document !== "undefined") {
  // The reader's own platform first. An Intel Mac cannot be told from an Apple
  // silicon one here, so the button says which Macs it is for.
  const here = /Win/.test(navigator.userAgent) ? "windows" : /Mac/.test(navigator.userAgent) ? "mac" : null;
  if (here) document.getElementById(`download-${here}`)?.classList.add("here");
  fill().catch(() => {});
}
