"""Generates Orbi's logo concepts (1024x1024, transparent) into concepts/."""
INK, CREAM, WARM, GOLD = "#0b0b0c", "#ecebe4", "#f3d9a6", "#e6b25e"
HEAD = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">'
def eyes(cx=512, cy=500, dx=150, r=80, fill=CREAM):
    return f'<circle cx="{cx-dx}" cy="{cy}" r="{r}" fill="{fill}"/><circle cx="{cx+dx}" cy="{cy}" r="{r}" fill="{fill}"/>'
concepts = {}
# A — Island: the face itself. Reads at 16 px.
concepts["a-island"] = HEAD + f'<rect x="72" y="292" width="880" height="440" rx="220" fill="{INK}"/>' + eyes() + '</svg>'
# B — Orbit: the island with a ring passing behind and in front, one moon.
ring = 'M -470 0 A 470 150 0 0 1 470 0'   # back (upper) half
front = 'M 470 0 A 470 150 0 0 1 -470 0'  # front (lower) half
concepts["b-orbit"] = HEAD + (
    f'<g transform="translate(512 512) rotate(-14)"><path d="{ring}" fill="none" stroke="{GOLD}" stroke-width="26" stroke-linecap="round" opacity=".55"/></g>'
    f'<rect x="112" y="322" width="800" height="380" rx="190" fill="{INK}"/>' + eyes(dx=140, r=72, cy=498) +
    f'<g transform="translate(512 512) rotate(-14)"><path d="{front}" fill="none" stroke="{GOLD}" stroke-width="26" stroke-linecap="round"/>'
    f'<circle cx="300" cy="116" r="40" fill="{WARM}"/></g></svg>')
# C — Notch: flat top, round bottom — Orbi hanging from the top of the screen.
concepts["c-notch"] = HEAD + (
    f'<rect x="72" y="250" width="880" height="34" rx="17" fill="{INK}" opacity=".35"/>'
    f'<path d="M160 250 H864 V520 A190 190 0 0 1 674 710 H350 A190 190 0 0 1 160 520 Z" fill="{INK}"/>' + eyes(cy=470, dx=140, r=72) + '</svg>')
# D — O: the letter as a ring, eyes inside, a moon on the rim.
concepts["d-o"] = HEAD + (
    f'<circle cx="512" cy="512" r="330" fill="none" stroke="{INK}" stroke-width="130"/>' + eyes(dx=95, r=58, cy=505, fill=INK) +
    f'<circle cx="786" cy="296" r="56" fill="{GOLD}"/></svg>')
# E — Dot matrix: the island built from a dot grid, two bright eyes.
dots = []
for row in range(7):
    for col in range(15):
        x, y = 120 + col * 56, 344 + row * 56
        # inside the stadium?
        cx = min(max(x, 120 + 168), 904 - 168)
        if (x - cx) ** 2 + (y - 512) ** 2 <= 172 ** 2:
            dots.append(f'<circle cx="{x}" cy="{y}" r="20" fill="{INK}"/>')
concepts["e-matrix"] = HEAD + "".join(dots) + eyes(dx=150, r=62, fill=GOLD) + '</svg>'
# F — Eclipse: an orb with a warm crescent behind it.
concepts["f-eclipse"] = HEAD + (
    f'<circle cx="560" cy="470" r="330" fill="{GOLD}" opacity=".9"/>'
    f'<circle cx="512" cy="512" r="330" fill="{INK}"/>' + eyes(dx=110, r=66, cy=500) + '</svg>')
for name, svg in concepts.items():
    open(f"concepts/{name}.svg", "w").write(svg + "\n")
print(len(concepts), "concepts")
