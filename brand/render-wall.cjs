// Renders brand/wallpaper.svg to the JPGs the landing page uses:  node render-wall.cjs
const { chromium } = require("playwright"); // run with NODE_PATH pointing at a global playwright
const fs = require("fs");
(async () => {
  const b = await chromium.launch();
  const p = await b.newPage({ viewport: { width: 2560, height: 1600 } });
  await p.setContent('<body style="margin:0">' + fs.readFileSync(__dirname + "/wallpaper.svg", "utf8").replace("<svg ", '<svg width="2560" height="1600" ') + "</body>");
  await p.screenshot({ path: __dirname + "/png/wallpaper-2560.jpg", type: "jpeg", quality: 86 });
  await p.setViewportSize({ width: 1440, height: 900 });
  await p.setContent('<body style="margin:0">' + fs.readFileSync(__dirname + "/wallpaper.svg", "utf8").replace("<svg ", '<svg width="1440" height="900" ') + "</body>");
  await p.screenshot({ path: __dirname + "/png/wallpaper-1440.jpg", type: "jpeg", quality: 84 });
  await b.close();
})();
