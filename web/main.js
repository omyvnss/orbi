// Orbi landing page: pixel mascots, eyes that follow the cursor on the ground
// islands, the hero capsule, one-click copy, and the "screen recording" of
// Orbi working with Claude Code on a MacBook.
(() => {
  const reduce = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const sleep = (ms) => new Promise((r) => setTimeout(r, reduce ? Math.min(ms, 120) : ms));

  // ------------------------------------------------------------ pixel mascots
  const PAL = {
    k: "#0d0d10", s: "#26262e", e: "#f4f1e8",
    L: "#2b2b31", l: "#4a4a53", c: "#8b7cf6",
    h: "#5a4a3e", H: "#3a2f28", g: "#141416", G: "#d9dde2",
    p: "#f3e6c8", P: "#dcc79a", b: "#b8742f",
  };
  const BODY = [
    "................", "...ssssssssss...", "..skkkkkkkkkkk..", "..skkkkkkkkkkk..",
    "..skkeekkkeekk..", "..skkeekkkeekk..", ".kkkkkkkkkkkkkk.", ".kkkkkkkkkkkkkk.",
    "..kkkkkkkkkkkk..", "..kkkkkkkkkkkk..",
  ];
  const LEGS_A = "..kk.kk..kk.kk..", LEGS_B = "...kk.kk..kk.kk.";
  const SLEEPY = (rows) => rows.map((r, y) => (y === 4 ? r.replace(/ee/g, "kk") : r));
  const WINK = (rows) => rows.map((r, y) => (y === 4 ? r.slice(0, 9) + r.slice(9).replace("ee", "kk") : r));
  const LAPTOP = ["LLLLLLLL", "LLLcLLLL", "LLcLcLLL", "LLLLLLLL", "lllllllll"];
  const HAT = ["....H......H....", "...hhhhhhhhhh...", "..hHHHHHHHHHHh..", ".hhhhhhhhhhhhhh."];
  const GLASS = [".ggg.", "gGGGg", "gGGGg", ".ggg.", "...g.", "....g"];
  const BOOK = ["..bbbbbbbbbbbb..", ".bppppppPpppppb.", "bpppppppPppppppb"];
  function svg(layers, w, h) {
    const grid = Array.from({ length: h }, () => Array(w).fill("."));
    for (const { rows, x = 0, y = 0 } of layers) {
      rows.forEach((row, ry) => [...row].forEach((ch, rx) => {
        const gx = x + rx, gy = y + ry;
        if (ch !== "." && gx >= 0 && gy >= 0 && gx < w && gy < h) grid[gy][gx] = ch;
      }));
    }
    let out = "";
    grid.forEach((row, y) => {
      let x = 0;
      while (x < w) {
        const ch = row[x];
        let run = 1;
        while (x + run < w && row[x + run] === ch) run++;
        if (ch !== ".") out += `<rect x="${x}" y="${y}" width="${run}" height="1" fill="${PAL[ch]}"/>`;
        x += run;
      }
    });
    return out;
  }
  const POSES = {
    walk: { w: 16, h: 12, f: [[{ rows: BODY, y: 1 }, { rows: [LEGS_A], y: 11 }], [{ rows: BODY }, { rows: [LEGS_B], y: 10 }]] },
    type: { w: 20, h: 13, f: [
      [{ rows: BODY, x: 3 }, { rows: [LEGS_A], x: 3, y: 10 }, { rows: LAPTOP, y: 7 }, { rows: ["kk"], x: 8, y: 7 }],
      [{ rows: BODY, x: 3, y: 1 }, { rows: [LEGS_A], x: 3, y: 11 }, { rows: LAPTOP, y: 8 }, { rows: ["kk"], x: 9, y: 8 }],
    ] },
    detective: { w: 22, h: 15, f: [
      [{ rows: BODY, y: 3 }, { rows: [LEGS_A], y: 13 }, { rows: HAT }, { rows: GLASS, x: 15, y: 6 }],
      [{ rows: WINK(BODY), y: 3 }, { rows: [LEGS_A], y: 13 }, { rows: HAT }, { rows: GLASS, x: 16, y: 5 }],
    ] },
    read: { w: 16, h: 14, f: [
      [{ rows: SLEEPY(BODY), y: 3 }, { rows: [LEGS_A], y: 13 }, { rows: BOOK, y: 1 }],
      [{ rows: SLEEPY(BODY), y: 4 }, { rows: [LEGS_A], y: 13 }, { rows: BOOK, y: 2 }],
    ] },
  };
  for (const el of document.querySelectorAll("[data-px]")) {
    const pose = POSES[el.dataset.px];
    if (!pose) continue;
    el.innerHTML = `<svg viewBox="0 0 ${pose.w} ${pose.h}" shape-rendering="crispEdges" aria-hidden="true">` +
      pose.f.map((layers, i) => `<g class="f${i + 1}">${svg(layers, pose.w, pose.h)}</g>`).join("") + "</svg>";
    el.style.setProperty("--ar", `${pose.w / pose.h}`);
  }

  // ------------------------------------------------------------ gaze (ground islands only)
  const islands = [...document.querySelectorAll("[data-island]")];
  let px = innerWidth / 2, py = innerHeight / 3, queued = false;
  const look = () => {
    queued = false;
    for (const el of islands) {
      const r = el.getBoundingClientRect();
      if (r.bottom < 0 || r.top > innerHeight) continue;
      const soft = (d) => d / (Math.abs(d) + 220);
      const max = r.height * 0.13;
      el.style.setProperty("--gx", `${(soft(px - (r.left + r.width / 2)) * max * 1.4).toFixed(2)}px`);
      el.style.setProperty("--gy", `${(soft(py - (r.top + r.height / 2)) * max).toFixed(2)}px`);
    }
  };
  if (!reduce) {
    addEventListener("pointermove", (e) => { px = e.clientX; py = e.clientY; if (!queued) { queued = true; requestAnimationFrame(look); } }, { passive: true });
  }

  // ------------------------------------------------------------ tap to open (touch)
  for (const el of document.querySelectorAll(".being, .bloom")) {
    el.addEventListener("click", (e) => { e.stopPropagation(); el.classList.toggle("open"); });
  }
  document.addEventListener("click", () => document.querySelectorAll(".being.open, .bloom.open").forEach((el) => el.classList.remove("open")));

  // ------------------------------------------------------------ copy install command
  const toast = document.querySelector(".toast");
  const command = `curl -fsSL ${location.origin}/install.sh | sh`;
  document.querySelectorAll("[data-copy]").forEach((el) =>
    el.addEventListener("click", async (e) => {
      e.preventDefault();
      let ok = false;
      try { await navigator.clipboard.writeText(command); ok = true; } catch { /* blocked */ }
      const target = el.querySelector("b") || el;
      const label = target.textContent;
      target.textContent = ok ? "Copied ✓" : "Copy below";
      setTimeout(() => { target.textContent = label; }, 1600);
      if (toast) toast.textContent = ok ? `Copied — paste in Terminal: ${command}` : command;
    }),
  );

  // ------------------------------------------------------------ hero capsule: a little life of its own
  const cap = document.querySelector("[data-capsule]");
  const capText = document.querySelector("[data-capsule-text]");
  const capIsland = cap?.querySelector(".island");
  if (cap && capText) {
    const LIFE = [
      ["is-working", "Claude Code is working…"],
      ["is-asking", "Codex wants to edit .env"],
      ["is-done", "Approved · tests passed"],
      ["", "Watching 3 agents"],
    ];
    let n = 0;
    const step = () => {
      const [state, text] = LIFE[n % LIFE.length];
      capText.classList.add("is-swap");
      setTimeout(() => {
        capText.textContent = text;
        capText.classList.remove("is-swap");
        cap.className = `capsule ${state}`;
        capIsland.className = `island island--cap ${state}`;
      }, 250);
      n += 1;
    };
    step();
    if (!reduce) setInterval(step, 3000);
  }

  // ------------------------------------------------------------ settings switches
  const setToggle = (t, on) => {
    t.setAttribute("aria-checked", String(on));
    const row = t.closest("[data-agent-row]");
    const label = row?.querySelector("[data-agent-state]");
    if (label) label.textContent = label.textContent.replace(/connected|off/, on ? "connected" : "off");
    row?.classList.toggle("is-on", on);
  };
  for (const t of document.querySelectorAll("[data-toggle]")) {
    t.addEventListener("click", () => setToggle(t, t.getAttribute("aria-checked") !== "true"));
  }

  // ------------------------------------------------------------ the recording
  const mac = document.querySelector("[data-dash]");
  if (!mac) return;
  const $ = (s) => mac.querySelector(s);
  const screen = $(".mac__screen");
  const log = $("[data-log]"), typed = $("[data-typed]");
  const line = $("[data-line]"), cmd = $("[data-cmd]"), cwd = $("[data-cwd]"), warn = $("[data-warn]");
  const keys = $("[data-keys]"), timer = $("[data-timer]"), bar = $("[data-bar]");
  const keycast = $("[data-keycast]"), pointer = $("[data-pointer]"), ripple = $("[data-ripple]");
  const island = $("[data-dash-island]");
  const counts = { allow: $("[data-n-allow]"), deny: $("[data-n-deny]"), back: $("[data-n-back]") };
  const demoToggle = $("[data-demo-toggle] [data-toggle]");
  const clock = $("[data-clock]");
  if (clock) { const d = new Date(); clock.textContent = d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" }); }
  // the recording timer in the menu bar, counting while the Mac is on screen
  const rec = $("[data-rec]");
  if (rec) {
    let secs = 0;
    setInterval(() => {
      if (!visible) return;
      secs++;
      rec.textContent = `${String(Math.floor(secs / 60)).padStart(2, "0")}:${String(secs % 60).padStart(2, "0")}`;
    }, 1000);
  }

  const SCENES = [
    { prompt: "run the tests before I push", say: "I'll run the test suite first.",
      tool: "Bash(<b>npm test</b>)", title: "Bash command", body: "npm test", desc: "Run the test suite",
      opt2: "Yes, and don't ask again for <b>npm test</b> commands in ~/code/orbi",
      line: "wants to run <code>npm test</code> in orbi/", detail: "npm test", warn: "", press: "allow",
      out: ["PASS  src/face.test.ts", "PASS  src/queue.test.ts", "Tests: 42 passed, 42 total"],
      after: "All 42 tests pass — ready to push." },
    { prompt: "clean the build folder and rebuild", say: "I'll clear dist/ and rebuild.",
      tool: "Bash(<b>rm -rf dist &amp;&amp; npm run build</b>)", title: "Bash command", body: "rm -rf dist &amp;&amp; npm run build", desc: "Remove the build output and rebuild",
      opt2: "Yes, and don't ask again for <b>rm</b> commands in ~/code/orbi",
      line: "wants to run <code>rm -rf dist</code> in orbi/", detail: "rm -rf dist && npm run build", warn: "deletes files", press: "deny",
      after: "Understood — I'll leave dist/ alone. Want a clean build into a new folder instead?" },
    { prompt: "split auth.ts into smaller files", say: "I'll move the session logic into its own file.",
      tool: "Update(<b>src/auth/session.ts</b>)", title: "Edit file", body: "src/auth/session.ts", desc: "+24 −11",
      opt2: "Yes, allow all edits during this session",
      line: "wants to edit <code>src/auth/session.ts</code> (+24 −11)", detail: "src/auth/session.ts\n+24 lines  −11 lines", warn: "", press: "allow",
      out: ["Updated src/auth/session.ts with 24 additions and 11 removals"],
      after: "Done — auth is three files now, and every import still resolves." },
  ];
  const n = { allow: 0, deny: 0, back: 0 };
  let visible = false, userAnswer = null, asking = false;

  const add = (html, cls) => {
    const p = document.createElement(cls === "cc-perm" ? "div" : "p");
    p.className = cls;
    p.innerHTML = html;
    log.appendChild(p);
    while (log.children.length > 18) log.firstElementChild.remove();
    return p;
  };
  const state = (s) => {
    mac.classList.remove("is-working", "is-asking", "is-done");
    island.classList.remove("is-working", "is-asking", "is-done");
    if (s) { mac.classList.add(`is-${s}`); island.classList.add(`is-${s}`); }
  };
  const until = async (cond) => { while (!cond()) await sleep(200); };
  const whenVisible = () => until(() => visible);

  // on phones the MacBook is wider than the screen: a camera follows the cursor,
  // until the visitor swipes — then it's theirs for a while
  const cam = mac.closest("[data-cam]");
  let touchedAt = 0;
  if (cam) ["touchstart", "pointerdown", "wheel"].forEach((ev) => cam.addEventListener(ev, () => { touchedAt = Date.now(); }, { passive: true }));
  const follow = (x) => {
    if (!cam || cam.scrollWidth <= cam.clientWidth + 4 || Date.now() - touchedAt < 9000) return;
    const offset = screen.getBoundingClientRect().left - cam.getBoundingClientRect().left + cam.scrollLeft;
    const left = Math.max(0, Math.min(cam.scrollWidth - cam.clientWidth, offset + x - cam.clientWidth / 2));
    cam.scrollTo({ left, behavior: reduce ? "auto" : "smooth" });
  };

  // pointer + clicks, positioned against the screen
  const pointAt = (el, fx = 0.5, fy = 0.5) => {
    const s = screen.getBoundingClientRect(), r = el.getBoundingClientRect();
    const x = r.left - s.left + r.width * fx, y = r.top - s.top + r.height * fy;
    pointer.style.setProperty("--x", `${x}px`);
    pointer.style.setProperty("--y", `${y}px`);
    ripple.style.setProperty("--x", `${x}px`);
    ripple.style.setProperty("--y", `${y}px`);
    follow(x);
  };
  const click = async () => { ripple.classList.remove("is-on"); void ripple.offsetWidth; ripple.classList.add("is-on"); await sleep(250); };
  const cast = async (label, ms = 900) => {
    keycast.innerHTML = label.split(" ").map((k) => `<kbd>${k}</kbd>`).join("");
    keycast.classList.add("is-on");
    await sleep(ms);
    keycast.classList.remove("is-on");
  };
  const type = async (text) => {
    typed.textContent = "";
    for (const ch of text) { typed.textContent += ch; await sleep(38 + Math.random() * 40); }
  };

  async function scene(sc) {
    await whenVisible();
    // 1 · type a prompt into Claude Code
    pointAt($(".cc__input"), 0.35, 0.5);
    await sleep(1000);
    await click();
    await type(sc.prompt);
    await sleep(250);
    await cast("⏎", 500);
    typed.textContent = "";
    add(`&gt; ${sc.prompt}`, "cc-user");
    // 2 · Claude works; Orbi notices
    state("working");
    line.innerHTML = "<b>Claude Code</b> is working…";
    const spin = add('<span class="cc-spin">✻</span> Thinking… <span class="cc-dim">(esc to interrupt)</span>', "cc-dim");
    const stars = ["✻", "✽", "✶", "✳", "✢"];
    for (let i = 0; i < 6; i++) { spin.firstElementChild.textContent = stars[i % stars.length]; await sleep(220); }
    spin.remove();
    add(`<span class="cc-dot">⏺</span>${sc.say}`, "");
    await sleep(500);
    add(`<span class="cc-dot">⏺</span>${sc.tool}`, "");
    // 3 · the permission prompt, exactly as Claude Code shows it — and Orbi asks
    const perm = add(
      `<p><b>${sc.title}</b></p><p>&nbsp;</p><p>&nbsp;&nbsp;${sc.body}</p><p class="cc-dim">&nbsp;&nbsp;${sc.desc}</p><p>&nbsp;</p>` +
      `<p>Do you want to proceed?</p><p class="cc-sel">❯ 1. Yes</p><p class="cc-dim">&nbsp;&nbsp;2. ${sc.opt2}</p><p class="cc-dim">&nbsp;&nbsp;3. No, and tell Claude what to do differently (esc)</p>`,
      "cc-perm");
    state("asking");
    line.innerHTML = `<b>Claude Code</b> ${sc.line}${sc.warn ? ` <em>· ${sc.warn}</em>` : ""}`;
    cmd.textContent = sc.detail;
    cwd.textContent = "in ~/code/orbi";
    warn.textContent = sc.warn;
    keys.hidden = false;
    timer.textContent = "then your terminal asks · 45s";
    bar.style.setProperty("--p", "100%");
    asking = true;
    userAnswer = null;
    // 4 · a glance at the island for details
    await sleep(500);
    pointAt(island, 0.5, 0.7);
    await sleep(900);
    mac.classList.add("is-expanded");
    bar.style.setProperty("--p", "86%");
    // give the visitor a moment to answer themselves
    for (let t = 0; t < 12 && !userAnswer; t++) await sleep(200);
    let answer = userAnswer;
    if (!answer) {
      answer = sc.press;
      await cast(answer === "allow" ? "⌃ ⌥ A" : "⌃ ⌥ D", 700);
    }
    asking = false;
    const btn = $(`[data-answer="${answer}"]`);
    btn?.classList.add("is-pressed");
    setTimeout(() => btn?.classList.remove("is-pressed"), 300);
    keys.hidden = true;
    bar.style.setProperty("--p", "0%");
    mac.classList.remove("is-expanded");
    perm.remove();
    n[answer] += 1;
    if (counts[answer]) counts[answer].textContent = n[answer];
    // 5 · Claude Code carries on
    if (answer === "allow") {
      state("done");
      line.innerHTML = "<b>Claude Code</b> approved · running";
      for (const o of sc.out || []) { await sleep(320); add(`⎿  ${o}`, "cc-out"); }
    } else {
      state(null);
      add("⎿  Denied in Orbi — the user said no", "cc-out");
    }
    await sleep(600);
    add(`<span class="cc-dot">⏺</span>${sc.after}`, "");
    await sleep(900);
    state(null);
    line.textContent = "Watching 2 agents";
  }

  async function settingsBeat(on) {
    await whenVisible();
    if (!demoToggle || getComputedStyle($(".win--set")).display === "none") return;
    pointAt(demoToggle, 0.5, 0.5);
    await sleep(1100);
    await click();
    setToggle(demoToggle, on);
    await sleep(700);
  }

  async function loop() {
    let i = 0;
    for (;;) {
      await scene(SCENES[i % SCENES.length]);
      if (i % SCENES.length === 0) await settingsBeat(true);
      if (i % SCENES.length === 2) await settingsBeat(false);
      await sleep(1200);
      i += 1;
    }
  }

  // take over any time: click Allow / Deny, or press the real hotkeys
  mac.addEventListener("click", (e) => {
    const b = e.target.closest("[data-answer]");
    if (b && asking) userAnswer = b.dataset.answer;
  });
  addEventListener("keydown", (e) => {
    if (!visible || !asking || !e.ctrlKey || !e.altKey) return;
    if (e.code === "KeyA") { e.preventDefault(); userAnswer = "allow"; }
    else if (e.code === "KeyD") { e.preventDefault(); userAnswer = "deny"; }
  });

  new IntersectionObserver(([entry]) => { visible = entry.isIntersecting; }, { threshold: 0.25 }).observe(mac);
  loop();
})();

// Orbi, reacting — the motion piece in the "why" card. One GSAP timeline, three
// scenes in turn: an agent works, asks, Orbi notices and chimes, shows the request
// in one line, and one key answers it. Sound is off until the visitor turns it on.
(() => {
  document.querySelectorAll("[data-origin]").forEach((el) => { el.textContent = location.origin; });
  const reel = document.querySelector("[data-reel]");
  if (!reel) return;
  const g = window.gsap;
  if (!g) { reel.classList.add("is-still"); return; }

  const $ = (s) => reel.querySelector(s), $$ = (s) => [...reel.querySelectorAll(s)];
  const sats = $$("[data-sat]"), moon = $("[data-moon]"), face = $("[data-face]"), ring = $("[data-ring]");
  const waves = $$("[data-wave]"), eyes = $$("[data-eye]"), eqWrap = $("[data-eq]"), eq = $$("[data-eq] b");
  const status = $("[data-status]"), card = $("[data-card]"), bar = $("[data-bar]"), keys = $$("[data-key]");
  const lastKey = $("[data-key-last]"), tether = $("[data-tether]"), soundBtn = $("[data-sound]");
  const agent = $("[data-agent]"), what = $("[data-what]"), warn = $("[data-warn]");
  const btns = { allow: $('[data-btn="allow"]'), deny: $('[data-btn="deny"]') };
  const count = { allow: $("[data-count-allow]"), deny: $("[data-count-deny]") };
  const GOLD = "#e2ad55", CREAM = "#ecebe4";

  // the agents orbit Orbi on the logo's tilted ring (stage units: 300 × 330)
  const CX = 150, CY = 120, TILT = (-14 * Math.PI) / 180, ct = Math.cos(TILT), st = Math.sin(TILT);
  const pt = (deg, rx, ry) => {
    const a = (deg * Math.PI) / 180, x = rx * Math.cos(a), y = ry * Math.sin(a);
    return { x: CX + x * ct - y * st, y: CY + x * st + y * ct, front: Math.sin(a) > 0 };
  };
  let unit = reel.clientWidth / 300, active = -1;
  const orbit = { a: 20, m: 0 }, boost = [1, 1, 1];
  const place = () => {
    sats.forEach((el, i) => {
      const p = pt(orbit.a + i * 120, 134, 44);
      el.style.transform = `translate(${p.x * unit}px, ${p.y * unit}px) scale(${(p.front ? 1 : 0.8) * boost[i]})`;
      el.style.zIndex = p.front ? 3 : 1;
      el.style.opacity = p.front || i === active ? 1 : 0.6;
      if (i === active) { tether.setAttribute("x2", p.x.toFixed(1)); tether.setAttribute("y2", p.y.toFixed(1)); }
    });
    const m = pt(orbit.m, 76, 24);
    moon.style.transform = `translate(${m.x * unit}px, ${m.y * unit}px)`;
    moon.style.zIndex = m.front ? 3 : 1;
  };
  new ResizeObserver(() => { unit = reel.clientWidth / 300; place(); }).observe(reel);
  const loops = [
    g.to(orbit, { a: 380, duration: 40, ease: "none", repeat: -1, onUpdate: place }),
    g.to(orbit, { m: 360, duration: 1.5, ease: "none", repeat: -1 }),
  ];

  // sound — synthesized, tiny, only after the visitor asks for it
  let ctx = null, soundOn = false;
  soundBtn.addEventListener("click", () => {
    soundOn = !soundOn;
    if (soundOn && !ctx) ctx = new (window.AudioContext || window.webkitAudioContext)();
    if (ctx) ctx.resume();
    soundBtn.setAttribute("aria-pressed", String(soundOn));
    soundBtn.querySelector("span").textContent = soundOn ? "sound on" : "sound off";
  });
  const tone = (f, at, dur, type = "sine", vol = 0.07) => {
    const o = ctx.createOscillator(), v = ctx.createGain();
    o.type = type; o.frequency.value = f;
    v.gain.setValueAtTime(0, at); v.gain.linearRampToValueAtTime(vol, at + 0.012); v.gain.exponentialRampToValueAtTime(0.0001, at + dur);
    o.connect(v).connect(ctx.destination); o.start(at); o.stop(at + dur + 0.05);
  };
  const SFX = {
    chime: (t) => { tone(880, t, 0.5); tone(1318.5, t + 0.12, 0.8); },
    key: (t) => tone(2400, t, 0.035, "triangle", 0.035),
    yes: (t) => { tone(659.3, t, 0.22); tone(987.8, t + 0.09, 0.45); },
    no: (t) => { tone(329.6, t, 0.2, "triangle", 0.06); tone(246.9, t + 0.11, 0.32, "triangle", 0.06); },
  };
  const play = (name) => { if (soundOn && ctx) SFX[name](ctx.currentTime + 0.01); };

  const setStatus = (text, gold = false) => g.to(status, { opacity: 0, duration: 0.12, overwrite: true, onComplete: () => {
    status.textContent = text; status.parentElement.classList.toggle("is-gold", gold); g.to(status, { opacity: 1, duration: 0.22 });
  } });
  const bump = (el) => { el.textContent = String(Number(el.textContent) + 1); g.fromTo(el, { yPercent: -70, opacity: 0 }, { yPercent: 0, opacity: 1, duration: 0.4 }); };

  const SCENES = [
    { sat: 1, agent: "Codex", verb: "run", what: "npm test", warn: "", answer: "allow" },
    { sat: 0, agent: "Claude Code", verb: "run", what: "rm -rf dist", warn: "deletes files", answer: "deny" },
    { sat: 2, agent: "OpenCode", verb: "run", what: "git push origin main", warn: "pushes code", answer: "allow" },
  ];
  let n = 0, sc = SCENES[0];

  const react = () => {
    const yes = sc.answer === "allow";
    g.to(btns[sc.answer], { backgroundColor: yes ? CREAM : "rgba(224, 83, 77, 0.22)", color: yes ? "#0b0b0c" : "#fff", duration: 0.18 });
    g.to(eyes, { backgroundColor: CREAM, duration: 0.2 });
    bump(count[sc.answer]);
    if (yes) {
      reel.classList.add("is-happy");
      g.fromTo(face, { y: 0 }, { y: -9 * unit, duration: 0.2, ease: "power2.out", yoyo: true, repeat: 1 });
      g.fromTo(waves[0], { opacity: 0.7, scale: 1, borderColor: CREAM }, { opacity: 0, scale: 1.9, duration: 1.1, ease: "power2.out" });
      setStatus(`approved · ${sc.agent} runs it`);
      play("yes");
    } else {
      reel.classList.add("is-cross");
      g.to(face, { keyframes: { x: [0, -7, 7, -5, 5, -2, 0].map((v) => v * unit) }, duration: 0.5, ease: "none" });
      setStatus(`denied · ${sc.agent} is told no`);
      play("no");
    }
  };

  const tl = g.timeline({ repeat: -1, paused: true, defaults: { duration: 0.5, ease: "power3.out" } });
  tl.call(() => {
    sc = SCENES[n++ % SCENES.length];
    agent.textContent = sc.agent; what.textContent = sc.what; warn.textContent = sc.warn;
    lastKey.textContent = sc.answer === "allow" ? "A" : "D";
    reel.classList.remove("is-happy", "is-cross");
    Object.values(btns).forEach((b) => g.set(b, { clearProps: "backgroundColor,color" }));
  }, null, 0);
  // idle: a blink
  tl.fromTo(eyes, { scaleY: 1 }, { scaleY: 0.1, duration: 0.08, ease: "power1.in", yoyo: true, repeat: 1 }, 0.6);
  // working: the agent lights up, the moon starts circling, Orbi glances around
  tl.call(() => {
    active = sc.sat; sats[active].classList.add("is-active");
    g.to(boost, { [active]: 1.3, duration: 0.5, ease: "power3.out" });
    setStatus(`${sc.agent} · working`);
  }, null, 1.1);
  tl.to(moon, { opacity: 1, duration: 0.3 }, 1.1);
  tl.to(eyes, { xPercent: -28, duration: 0.4 }, 1.5).to(eyes, { xPercent: 28, duration: 0.5 }, 2.2).to(eyes, { xPercent: 0, duration: 0.4 }, 2.8);
  // asking: squash, the gold ring, a chime you can see
  tl.to(moon, { opacity: 0, duration: 0.2 }, 3.1);
  tl.fromTo(face, { scaleX: 1, scaleY: 1 }, { keyframes: [
    { scaleX: 1.1, scaleY: 0.86, duration: 0.12, ease: "power2.in" },
    { scaleX: 0.97, scaleY: 1.06, duration: 0.18, ease: "power2.out" },
    { scaleX: 1, scaleY: 1, duration: 0.45, ease: "power3.out" },
  ] }, 3.2);
  tl.call(() => { setStatus(`${sc.agent} needs you`, true); play("chime"); }, null, 3.22);
  tl.fromTo(ring, { opacity: 0, scale: 1.18 }, { opacity: 1, scale: 1, duration: 0.45 }, 3.25);
  tl.to(eyes, { backgroundColor: "#efc57a", duration: 0.25 }, 3.25);
  tl.fromTo(waves, { opacity: 0.8, scale: 1, borderColor: GOLD }, { opacity: 0, scale: 2.3, duration: 1.5, ease: "power2.out", stagger: 0.22 }, 3.3);
  tl.fromTo(tether, { opacity: 0 }, { opacity: 0.85, duration: 0.4 }, 3.3);
  tl.fromTo(eqWrap, { opacity: 0 }, { opacity: 1, duration: 0.2 }, 3.25);
  tl.fromTo(eq, { scaleY: 0.25 }, { scaleY: (i) => [0.9, 0.5, 1, 0.6, 0.8][i], duration: 0.14, ease: "power1.out", yoyo: true, repeat: 7, stagger: 0.04 }, 3.3);
  tl.to(eqWrap, { opacity: 0, duration: 0.3 }, 5.1);
  // the one line
  tl.fromTo(card, { opacity: 0, y: 14, clipPath: "inset(0% 0% 100% 0% round 10px)" }, { opacity: 1, y: 0, clipPath: "inset(0% 0% 0% 0% round 10px)", duration: 0.65 }, 3.7);
  tl.fromTo(bar, { scaleX: 1 }, { scaleX: 0.62, duration: 2.6, ease: "none" }, 4.2);
  // one key
  tl.fromTo(keys, { opacity: 0, y: -18 }, { opacity: 1, y: 0, duration: 0.45, stagger: 0.08 }, 5.4);
  tl.to(keys, { y: 3, boxShadow: "0 0px 0 #050507", duration: 0.08, ease: "power2.in" }, 6.3);
  tl.call(() => { play("key"); react(); }, null, 6.36);
  tl.to(keys, { y: 0, boxShadow: "0 3px 0 #050507", duration: 0.22 }, 6.42);
  // resolve and settle back to watching
  tl.to(ring, { opacity: 0, scale: 1.08, duration: 0.5 }, 6.7);
  tl.to(tether, { opacity: 0, duration: 0.4 }, 6.9);
  tl.to(card, { opacity: 0, y: -8, duration: 0.4, ease: "power2.in" }, 7.3);
  tl.to(keys, { opacity: 0, y: 10, duration: 0.35, stagger: 0.05, ease: "power2.in" }, 7.3);
  tl.call(() => {
    sats.forEach((s) => s.classList.remove("is-active"));
    g.to(boost, { 0: 1, 1: 1, 2: 1, duration: 0.6 });
  }, null, 7.7);
  tl.call(() => { reel.classList.remove("is-happy", "is-cross"); active = -1; setStatus("watching 3 agents"); }, null, 8.9);
  tl.to({}, { duration: 0.01 }, 10);

  if (matchMedia("(prefers-reduced-motion: reduce)").matches) {
    loops.forEach((t) => t.pause()); place(); tl.seek(4.6, true); return;
  }
  new IntersectionObserver(([e]) => {
    if (e.isIntersecting) { tl.play(); loops.forEach((t) => t.play()); }
    else { tl.pause(); loops.forEach((t) => t.pause()); }
  }, { threshold: 0.2 }).observe(reel);
})();

// Entrance and scroll-in — smooth, not bouncy: power3/expo outs, short staggers.
(() => {
  const root = document.documentElement;
  const g = window.gsap;
  if (!g || matchMedia("(prefers-reduced-motion: reduce)").matches) { root.classList.add("rise-done"); return; }
  root.classList.add("rise-done");
  const tl = g.timeline({ defaults: { ease: "power3.out", duration: 0.9 } });
  tl.fromTo(".bar", { y: -18, opacity: 0 }, { y: 0, opacity: 1, duration: 0.8 }, 0)
    .fromTo(".sol", { opacity: 0, scale: 0.94 }, { opacity: 1, scale: 1, duration: 1.4, ease: "expo.out" }, 0.1)
    .fromTo(".pitch > *", { opacity: 0, y: 22 }, { opacity: 1, y: 0, stagger: 0.09 }, 0.2)
    .fromTo(".capsule", { opacity: 0 }, { opacity: 1, duration: 0.7 }, 0.7)
    .fromTo(".note, .caps", { opacity: 0 }, { opacity: 1, duration: 1.1, ease: "power2.out", stagger: 0.06 }, 0.6);

  // below the fold: rise once, when a piece first comes into view
  const pieces = [".card--one .card__tag", ".card--one .head", ".card--one .body", ".facts li", ".reel",
    "#try .lede", "#try .small", ".mac-scroll", ".card--two .card__tag", ".manifesto span", ".rules li",
    ".install .card__tag", ".install__h", ".install__sub", ".way", ".foot .lede", ".foot__grid"];
  const els = pieces.flatMap((s) => [...document.querySelectorAll(s)]);
  g.set(els, { opacity: 0, y: 26 });
  const io = new IntersectionObserver((entries) => {
    const now = entries.filter((e) => e.isIntersecting).map((e) => e.target);
    if (!now.length) return;
    now.forEach((el) => io.unobserve(el));
    g.to(now, { opacity: 1, y: 0, duration: 0.9, ease: "power3.out", stagger: 0.07, overwrite: true });
  }, { rootMargin: "0px 0px -8% 0px" });
  els.forEach((el) => io.observe(el));
})();
