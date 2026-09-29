// Renders SVGs to transparent PNGs:  node render.cjs '[["in.svg", 1024, "out.png"], ...]'
const { chromium } = require("playwright"); // run with NODE_PATH pointing at a global playwright
const fs = require("fs");
(async () => {
  const jobs = JSON.parse(process.argv[2]);
  const b = await chromium.launch();
  const p = await b.newPage();
  for (const [svg, size, out] of jobs) {
    await p.setViewportSize({ width: size, height: size });
    const src = fs.readFileSync(svg, "utf8");
    await p.setContent('<html><body style="margin:0;background:transparent">' + src.replace("<svg ", '<svg width="' + size + '" height="' + size + '" ') + "</body></html>");
    await p.screenshot({ path: out, omitBackground: true, clip: { x: 0, y: 0, width: size, height: size } });
  }
  await b.close();
})();
