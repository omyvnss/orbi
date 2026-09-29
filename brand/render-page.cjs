// Renders HTML templates to PNG:  NODE_PATH=<global node_modules> node render-page.cjs templates/jobs.json
const { chromium } = require("playwright");
const path = require("path");
(async () => {
  const jobs = require(path.resolve(process.argv[2]));
  const b = await chromium.launch();
  for (const [html, w, h, out] of jobs) {
    const p = await b.newPage({ viewport: { width: w, height: h } });
    await p.goto("file://" + path.resolve(html), { waitUntil: "networkidle" });
    await p.evaluate(() => document.fonts.ready);
    await p.screenshot({ path: out });
    await p.close();
  }
  await b.close();
})();
