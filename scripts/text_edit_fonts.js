// Loaded only by the generated Phase 5 spike page, never by the application.
// A resolved FontFace.load() is not evidence that a character used that font:
// advances AND deliberately distinctive interior pixels must match the fixture.
async function runFontProbe() {
  const cases = JSON.parse(document.getElementById('cases').textContent);
  const reports = [];
  for (const entry of cases) {
    const family = `Probe${reports.length}`;
    const face = new FontFace(family, Uint8Array.from(atob(entry.data), c => c.charCodeAt(0)).buffer);
    let loaded = false;
    let error = '';
    try {
      await face.load();
      document.fonts.add(face);
      loaded = true;
    } catch (e) {
      error = String(e);
    }
    const canvas = document.createElement('canvas');
    canvas.width = 220;
    canvas.height = 120;
    const ctx = canvas.getContext('2d', { willReadFrequently: true });
    ctx.fillStyle = 'white';
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.fillStyle = 'black';
    ctx.font = `100px ${family}, monospace`;
    ctx.fontKerning = 'none';
    ctx.fillText('AB', 10, 90);
    const width = ctx.measureText('AB').width;
    const dark = (x, y) => ctx.getImageData(x, y, 1, 1).data[0] < 32;
    // Known outlines: A at x=10..50, B at 70..80 and 100..110.
    const pixels = dark(30, 50) && dark(75, 50) && !dark(90, 50) && dark(105, 50)
      && !dark(55, 50) && !dark(30, 10) && !dark(30, 100);
    const paintsCorrectly = loaded && Math.abs(width - 120) < 0.01 && pixels;
    const label = document.createElement('p');
    label.textContent = entry.name;
    document.getElementById('samples').append(label, canvas);
    document.fonts.delete(face);
    reports.push({ name: entry.name, loaded, width, paintsCorrectly,
      candidate: entry.candidate, missing: entry.missing, rights: entry.rights, error });
  }
  const expectedNames = ['truetype', 'opentype-cff', 'missing-cmap', 'missing-rights',
    'restricted', 'preview-print', 'editable', 'no-subsetting', 'bitmap-only',
    'conflicting-rights', 'reserved-rights', 'raw-cff', 'invalid-font'];
  const failures = [];
  if (JSON.stringify(reports.map(r => r.name)) !== JSON.stringify(expectedNames)) failures.push('fixture population');
  for (const report of reports) {
    if (report.candidate && !report.paintsCorrectly) failures.push(`${report.name}: candidate did not paint faithfully`);
    if (report.candidate && JSON.stringify(report.missing) !== '["Z"]') failures.push(`${report.name}: missing glyph not reported`);
    if (report.name === 'invalid-font' && report.loaded) failures.push('invalid font was accepted');
    if (report.name === 'missing-cmap' && report.paintsCorrectly) failures.push('missing cmap unexpectedly reproduced both glyphs');
  }
  return { passed: failures.length === 0, failures, userAgent: navigator.userAgent, reports };
}

runFontProbe().then(result => {
  document.getElementById('result').textContent = JSON.stringify(result, null, 2);
  window.fontProbeResult = result;
}).catch(error => {
  window.fontProbeResult = { passed: false, failures: [String(error)] };
  document.getElementById('result').textContent = JSON.stringify(window.fontProbeResult);
});
