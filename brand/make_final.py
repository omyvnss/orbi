"""Orbi's final logo system. Writes:
  orbi-mark.svg        primary mark (Orbit), for light backgrounds, transparent
  orbi-mark-light.svg  primary mark for dark backgrounds, transparent
  orbi-glyph.svg       small-size mark (Island), transparent
  favicon.svg          Island glyph that flips for dark browser tabs
  orbi-app-icon.svg    macOS app icon (sage squircle)
  orbi-tray.svg        menu-bar template (black silhouette, eyes cut out)
"""
INK, CREAM, WARM, GOLD, SAGE_T, SAGE_B = "#0b0b0c", "#ecebe4", "#f6e2b8", "#e2ad55", "#e4e5d8", "#c3c5b2"
H = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">'
BACK, FRONT = "M -440 0 A 440 138 0 0 1 440 0", "M 440 0 A 440 138 0 0 1 -440 0"

def orbit(pill, eye, ring_back_opacity=".5", defs=""):
    return (H + defs +
        f'<g transform="translate(512 512) rotate(-14)"><path d="{BACK}" fill="none" stroke="url(#ring)" stroke-width="30" stroke-linecap="round" opacity="{ring_back_opacity}"/></g>'
        f'<rect x="142" y="336" width="740" height="352" rx="176" fill="{pill}"/>'
        f'<circle cx="382" cy="500" r="66" fill="{eye}"/><circle cx="642" cy="500" r="66" fill="{eye}"/>'
        f'<g transform="translate(512 512) rotate(-14)"><path d="{FRONT}" fill="none" stroke="url(#ring)" stroke-width="30" stroke-linecap="round"/>'
        f'<circle cx="286" cy="104" r="42" fill="{WARM}"/></g></svg>\n')
ring = f'<defs><linearGradient id="ring" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="{GOLD}"/><stop offset=".6" stop-color="#efc57a"/><stop offset="1" stop-color="{WARM}"/></linearGradient></defs>'
files = {}
files["orbi-mark.svg"] = orbit(INK, CREAM, defs=ring)
files["orbi-mark-light.svg"] = orbit(CREAM, INK, ring_back_opacity=".6", defs=ring)
glyph = lambda pill, eye: f'<rect x="72" y="292" width="880" height="440" rx="220" fill="{pill}"/><circle cx="362" cy="500" r="84" fill="{eye}"/><circle cx="662" cy="500" r="84" fill="{eye}"/>'
files["orbi-glyph.svg"] = H + glyph(INK, CREAM) + "</svg>\n"
files["favicon.svg"] = (H + '<style>.p{fill:%s}.e{fill:%s}@media (prefers-color-scheme:dark){.p{fill:%s}.e{fill:%s}}</style>' % (INK, CREAM, CREAM, INK) +
    '<rect class="p" x="40" y="272" width="944" height="480" rx="240"/><circle class="e" cx="352" cy="500" r="96"/><circle class="e" cx="672" cy="500" r="96"/></svg>\n')
files["orbi-tray.svg"] = (H + '<mask id="m"><rect width="1024" height="1024" fill="#fff"/><circle cx="352" cy="500" r="96" fill="#000"/><circle cx="672" cy="500" r="96" fill="#000"/></mask>'
    '<rect x="40" y="272" width="944" height="480" rx="240" fill="#000" mask="url(#m)"/></svg>\n')
files["orbi-app-icon.svg"] = (H + f'''<defs>
  <linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{SAGE_T}"/><stop offset="1" stop-color="{SAGE_B}"/></linearGradient>
  <radialGradient id="glow" cx=".5" cy=".42" r=".55"><stop offset="0" stop-color="#fbfbf4" stop-opacity=".75"/><stop offset="1" stop-color="#fbfbf4" stop-opacity="0"/></radialGradient>
  <linearGradient id="pill" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#16161a"/><stop offset="1" stop-color="#060607"/></linearGradient>
  <linearGradient id="sheen" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".14"/><stop offset=".55" stop-color="#fff" stop-opacity="0"/></linearGradient>
  <radialGradient id="eye" cx=".38" cy=".32" r=".75"><stop offset="0" stop-color="#ffffff"/><stop offset=".45" stop-color="#f1efe6"/><stop offset="1" stop-color="#c9c6ba"/></radialGradient>
  <linearGradient id="ring" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="{GOLD}"/><stop offset=".6" stop-color="#efc57a"/><stop offset="1" stop-color="{WARM}"/></linearGradient>
  <filter id="shadow" color-interpolation-filters="sRGB" x="-20%" y="-20%" width="140%" height="140%"><feDropShadow dx="0" dy="14" stdDeviation="16" flood-color="#000" flood-opacity=".32"/></filter>
  <filter id="blur" x="-50%" y="-200%" width="200%" height="500%"><feGaussianBlur stdDeviation="18"/></filter>
  <filter id="soft" x="-30%" y="-30%" width="160%" height="160%"><feDropShadow dx="0" dy="18" stdDeviation="22" flood-color="#1a1a14" flood-opacity=".35"/></filter>
  <filter id="moon" x="-100%" y="-100%" width="300%" height="300%"><feGaussianBlur stdDeviation="10" result="b"/><feMerge><feMergeNode in="b"/><feMergeNode in="SourceGraphic"/></feMerge></filter>
</defs>
<rect x="100" y="100" width="824" height="824" rx="186" fill="url(#bg)" filter="url(#shadow)"/>
<rect x="100" y="100" width="824" height="824" rx="186" fill="url(#glow)"/>
<rect x="101.5" y="101.5" width="821" height="821" rx="184.5" fill="none" stroke="#fff" stroke-opacity=".55" stroke-width="3"/>
<g transform="translate(512 520) rotate(-14)"><path d="M -318 0 A 318 100 0 0 1 318 0" fill="none" stroke="url(#ring)" stroke-width="20" stroke-linecap="round" opacity=".55"/></g>
<ellipse cx="512" cy="652" rx="250" ry="34" fill="#1a1a14" opacity=".28" filter="url(#blur)"/>
<rect x="244" y="388" width="536" height="258" rx="129" fill="url(#pill)"/>
<rect x="244" y="388" width="536" height="258" rx="129" fill="url(#sheen)"/>
<circle cx="418" cy="508" r="48" fill="url(#eye)"/><circle cx="606" cy="508" r="48" fill="url(#eye)"/>
<g transform="translate(512 520) rotate(-14)"><path d="M 318 0 A 318 100 0 0 1 -318 0" fill="none" stroke="url(#ring)" stroke-width="20" stroke-linecap="round"/>
<circle cx="208" cy="76" r="30" fill="{WARM}" filter="url(#moon)"/></g>
</svg>
''')
for name, svg in files.items():
    open(name, "w").write(svg)
print("\n".join(files))
