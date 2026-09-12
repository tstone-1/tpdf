/** Optional native-window checks, excluded from normal builds at compile time. */
export async function runAutobenchIfRequested(...args: Parameters<typeof import("./autobench").runAutobenchIfRequested>) {
  if (!__TPDF_CHECKS__) return false;
  return (await import("./autobench")).runAutobenchIfRequested(...args);
}
export async function runScrollBenchIfRequested(...args: Parameters<typeof import("./scrollbench").runScrollBenchIfRequested>) {
  if (!__TPDF_CHECKS__) return false;
  return (await import("./scrollbench")).runScrollBenchIfRequested(...args);
}
export async function runStartupTimelineIfRequested(...args: Parameters<typeof import("./startup").runStartupTimelineIfRequested>) {
  if (!__TPDF_CHECKS__) return false;
  return (await import("./startup")).runStartupTimelineIfRequested(...args);
}
export async function runViewerCheckIfRequested(...args: Parameters<typeof import("./viewercheck").runViewerCheckIfRequested>) {
  if (!__TPDF_CHECKS__) return false;
  return (await import("./viewercheck")).runViewerCheckIfRequested(...args);
}
export async function runMarkCheckIfRequested(...args: Parameters<typeof import("./markcheck").runMarkCheckIfRequested>) {
  if (!__TPDF_CHECKS__) return false;
  return (await import("./markcheck")).runMarkCheckIfRequested(...args);
}
export async function runSessionCheckIfRequested(...args: Parameters<typeof import("./sessioncheck").runSessionCheckIfRequested>) {
  if (!__TPDF_CHECKS__) return false;
  return (await import("./sessioncheck")).runSessionCheckIfRequested(...args);
}
export async function runOpenCheckIfRequested(...args: Parameters<typeof import("./opencheck").runOpenCheckIfRequested>) {
  if (!__TPDF_CHECKS__) return false;
  return (await import("./opencheck")).runOpenCheckIfRequested(...args);
}
