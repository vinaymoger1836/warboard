# 10 — UI style guide

The look of WarBoard is defined by the reference screenshots in `docs/ui-reference/`. This guide turns them into rules and tokens. Colours below were sampled from the screenshots.

**Precedence:** this guide → the screenshots → anything else. Where screenshots disagree with each other, this guide says which one wins.

## 1. Reference screenshots

| File | Take from it |
|---|---|
| `01-strike-route-baltic.png` | Coalition territory tints on land **and** territorial waters, territory outlines, ship hexagon, squadron tiles with "SQN", battalion stack with "II" marks, SITE icons (air base, naval base, S-400), waypoints "WP 1–3", amber route, dashed strike lines, salvo label "JSM mk1 (15× salvo)", "(our estimate)" second line |
| `02-salvo-vs-ships.png` | Salvo leaving a ship toward two targets, amber target lines. **Its cyan outgoing trail is superseded by 04:** offensive weapons are amber |
| `03-ships-targeted.png` | Red glow halo on targeted units, amber selection ring, sensor range arc, enemy-tinted sea |
| `04-interceptors.png` | Offensive missiles amber with `M-n` tags, interceptors cyan dashed with `INT-n` tags, group labels "SHIP FREMM class (5× interceptors)" |
| `05-unit-card-rings.png` | Unit card layout, the three range rings and their colours, combat-radius label, country labels, dark basemap |
| `06-world-satellites.png` | World view, satellite dot + ground tracks + yellow label, enemy missile red label with speed, enemy territory tint and outline, city dots, "OPEN 3D TACTICAL VIEW" button, "Simulation paused" toast, scale bar, zoom buttons |

## 2. Tokens

Use these names everywhere (CSS variables in `apps/game`, RGBA arrays for deck.gl and Cesium in `packages/symbols/tokens.ts`).

### 2.1 Map

| Token | Hex | Use |
|---|---|---|
| `map.sea` | `#2C343C` | Sea |
| `map.land` | `#0E0E0E` | Land (neutral) |
| `map.border` | `#3A4048` | Country borders, thin |
| `map.coast` | `#3A4048` | Coastline |
| `map.label.country` | `#E6E8EB` | Country names, uppercase |
| `map.label.sea` | `#5E6E86` | Sea names, italic uppercase |
| `map.label.city` | `#E6E8EB` | City names, italic |
| `map.city.dot` | `#C9A227` | City dot |
| `terr.own.fill` | `#423662` | Own coalition land and 12 nm waters (opaque look, see §3) |
| `terr.own.line` | `#544C7C` | Own territory outline |
| `terr.enemy.fill` | `#284A62` | Enemy coalition land and 12 nm waters |
| `terr.enemy.line` | `#4C98CC` | Enemy territory outline |

### 2.2 Units, weapons, overlays

| Token | Hex | Use |
|---|---|---|
| `side.own` | `#886CD4` | Own icon fill (ships, sites, squadrons); "SQN" text `#755EB3` |
| `side.enemy` | `#4C9CD4` | Enemy icon fill |
| `side.neutral` | `#8C949C` | Neutral units |
| `side.unknown` | `#ECB030` | Unknown contacts (identity not established) |
| `glyph` | `#FFFFFF` | Glyph inside icons |
| `weapon.offensive` | `#FCB020` | Our missiles, bombs, routes, selection ring |
| `weapon.line` | `#ECB030` | Dashed shooter→target lines |
| `weapon.interceptor` | `#4CD0E0` | Interceptors (any side's SAMs in flight) |
| `weapon.enemy` | `#D8504C` | Enemy weapons in our picture; label `#E0504C` |
| `halo.targeted` | `#A05460` @ 45% | Glow around a unit being targeted |
| `ring.sensor` | `#4CC0F4` | Radar / sensor horizon ring and its card row |
| `ring.engage` | `#E08804` | Max engagement range ring and row |
| `ring.radius` | `#A058AC` | Combat radius ring; text `#B868C8` |
| `ring.threat` | `#D8504C` | Estimated enemy SAM threat ring |
| `sat` | `#FFD54F` | Satellite dot; label `#E9C34A`; ground track `#B49C48` dashed |
| `waypoint` | `#886CD4` stroke, label `#FFFFFF` | Route waypoints |

### 2.3 Panels and chrome

| Token | Hex | Use |
|---|---|---|
| `panel.bg` | `#0A101A` | Unit card, panels |
| `panel.header` | `#0E1E2C` | Card header band |
| `panel.inset` | `#060C14` | Ranges box inside the card |
| `panel.border` | `#245470` | 1 px panel border |
| `text.primary` | `#FFFFFF` | Titles, values |
| `text.secondary` | `#8C9AAA` | Labels, button text on chrome |
| `text.muted` | `#586675` | Subtitles ("643 Tactical Formation · 12 Personnel") |
| `state.ok` | `#4CA85C` | Fuel ≥ 50%, healthy |
| `state.warn` | `#FCB020` | Fuel 25–50%, RTB, approvals |
| `state.bad` | `#E03C68` | Bingo, damaged, lost |
| `state.station` | `#B868C8` | "ON STATION" |
| `btn.border` / `btn.text` | `#FCB020` | Command buttons (outlined) |
| `chrome.bg` | `#0C141C` | "OPEN 3D TACTICAL VIEW", zoom buttons |
| `toast.bg` / `toast.text` | `#080E16` / `#C4CED6` | Toasts ("Simulation paused") |

## 3. Map

- Dark basemap from the Protomaps vector tiles, restyled with the map tokens. No light theme on the map.
- **Territory tints** cover land **and** the 12 nm territorial sea (the purple band along the coast in 01). The tint replaces the base land/sea colour inside the territory; the outline is drawn on top in the territory line colour.
- Labels: countries uppercase, letter-spaced 0.12em, bold; seas italic uppercase muted; cities italic with an amber-gold dot. All map labels get a 2 px dark halo (`#0E0E0E`).
- Chrome on the map: zoom `+`/`−` square buttons bottom right, scale bar above them (white box, dark text), layers button on the right edge (round), **OPEN 3D TACTICAL VIEW** top right (§9).

## 4. Unit symbols

Icons are SVG in `packages/symbols`, packed into one atlas used by both 2D and 3D. Size 30 px at default zoom, 24 px when zoomed out. Fill = side colour, glyph white, 1 px stroke one step darker than the fill. **No emoji anywhere.**

| Category | Frame | Glyph | Marks above |
|---|---|---|---|
| Ship / submarine | Flat-top hexagon (wider than tall) | Ship / submarine | — |
| Aircraft flight / squadron | Rounded tile with a small point at the bottom | Aircraft type silhouette (fighter, bomber, tanker, AEW, transport, helicopter) | `SQN` |
| Drone | Same tile | Drone | `SQN` |
| Battalion-size system (Patriot, S-400) | Two overlapping tiles (launcher + radar) | Launcher, radar arc | `II` |
| Site (air base, naval base, SAM, radar, HQ) | Rounded hexagon, pointy top | Runway + aircraft, anchor, radar arc, antenna, flag | — |
| Ground formation | Chevron tile | Armour, infantry, artillery, SOF, airborne, marines | Echelon (`I`, `II`, `III`, `X`) |
| Satellite | Yellow dot | — | — |

### 4.1 Labels

Italic 11 px, light grey `#D9D9D9` with dark halo, centred under the icon.

| Case | Format | Example |
|---|---|---|
| Ship | `SHIP <class or name>` | `SHIP FREMM class` |
| Flight / squadron | `<n> × <type>` (count and type) or `SQN <type>` for a single airframe | `3 × Rafale`, `SQN MQ-9A Reaper` |
| Battalion | `BN <system>` | `BN MIM-104 Patriot PAC-3` |
| Site | `SITE <name>` | `SITE Air base`, `SITE Naval base` |
| Estimated contact (below *Precise* or stale) | Second line `(our estimate)`; dashed frame at 60% opacity; uncertainty ellipse dashed | `SITE S-400` / `(our estimate)` |
| Unknown contact | `UNKNOWN AIR` / `UNKNOWN SURFACE`, amber frame | — |
| Satellite | Bold yellow, 2 lines: owner + name, then `[<payload> <swath>km]` | `USA KH-11 Block IV Keyhole` / `[OPTICAL 460km]` |
| Enemy weapon | Red, 2 lines: type, then speed | `9M96 (Redut)` / `(4288 km/h)` |

At low zoom, groups collapse into one icon with the count in the label. Labels are hidden first when crowded; icons never are.

## 5. Weapons and effects

| Element | Rule |
|---|---|
| Our missile | Small amber missile glyph with a 1.5 px amber trail |
| Salvo label | At the salvo's centroid: `<type> (<n>× salvo)`, e.g. `JSM mk1 (12× salvo)` |
| Per-missile tags | `M-1 … M-n` from zoom 7, next to each missile |
| Shooter→target line | Dashed amber while weapons fly (01, 04) |
| Interceptors | Cyan dashed trail with a ring-dot head; tags `INT-1 … INT-n`; group label `SHIP FREMM class (5× interceptors)` |
| Enemy weapons | Red glyph and red label with speed (06) |
| Launch | Small white flash at the shooter |
| Intercept | Cyan burst |
| Hit | Amber-white burst, then the target's damage state on its icon |
| Targeted unit | Red glow halo (`halo.targeted`), ~34 px radius, pulsing slowly |
| Selected unit | 2 px amber ring around the icon |

## 6. Routes, orders and rings

- **Waypoints:** hollow 10 px circle in the side colour, white bold label `WP 1`, `WP 2` …; route line solid amber 2 px.
- **Patrol circle:** dashed side colour, 1.5 px.
- **Range rings** for the selected unit, all dashed 2 px: sensor `ring.sensor`, engagement `ring.engage`, combat radius `ring.radius` (1.5 px). The combat radius ring carries a label with a small purple dot icon: `<Base> Combat Radius (1390 km)`.
- **Threat rings** of identified enemy SAMs: dashed `ring.threat`, label `(est.)`.
- Rings are true geodesic circles (a 1,390 km ring is not a screen circle on Mercator).

## 7. Unit card (from 05)

Floating, bottom right, width `min(470px, 100vw − 32px)`, `panel.bg`, 1 px `panel.border`, radius 10 px.

```
┌──────────────────────────────────────────────────────┐
│ [type icon]  12 × Eurofighter Typhoon            [×] │  header band, title 15 px semibold
│              643 Tactical Formation · 12 Personnel   │  12 px text.muted
├──────────────────────────────────────────────────────┤
│ Status: ON STATION                       Fuel: 95%   │  state colours (§2.3)
│ Speed: 2495 km/h    Altitude: 7.0 km   Heading: 214° │  label secondary, value bold
│ ┌──────────────────────────────────────────────────┐ │  panel.inset
│ │ ◉ Radar / Sensor Horizon:                 250 km │ │  ring.sensor
│ │ ◉ Max Engagement Range:                   200 km │ │  ring.engage
│ │ ◉ Combat Radius Reach:                   1390 km │ │  ring.radius text
│ └──────────────────────────────────────────────────┘ │
│ ▸ Task · Weapons · Sensors & EMCON · ROE · Specs     │  collapsible sections (new)
│ [ ORDER RTB ]                 [ RETASK PATROL ]      │  outlined amber, mono, uppercase
└──────────────────────────────────────────────────────┘
```

- Row icons are SVG glyphs from the icon set, not emoji (the reference uses emoji; this is the one deliberate change).
- Each range row also toggles its ring on the map.
- Status colours: ON STATION `state.station`, PATROL / TRANSIT `text.primary`, RTB `state.warn`, ENGAGING `weapon.enemy`, LANDED / TURNAROUND `text.secondary`.
- Fuel colour: ≥ 50% `state.ok`, 25–50% `state.warn`, < 25% or bingo `state.bad`.
- Buttons by unit type, two per row: aircraft `ORDER RTB`, `RETASK PATROL`, `STRIKE`, `HOLD FIRE`; ships `MOVE`, `PATROL`, `STRIKE`, `EMCON`; sites `EMCON`, `HOLD FIRE`. Button text: JetBrains Mono 12 px, letter-spacing 0.12em.
- Contact cards use the same frame, title `<label> (our estimate)` when estimated, plus classification level, last update age, uncertainty (± km) and sources.

## 8. Other panels

Not in the screenshots; build them in the same family:

- **Top bar:** `panel.bg`; clock in JetBrains Mono (`03:40:12Z  T+0:00:12`); speed buttons `⏸ ×1 ×3 ×5 ×10 ×30 ×60` as a segmented control (active = amber outline); achieved speed in `text.secondary`; auto-slow toggle; side ROE `A B C` segmented.
- **Left panel (forces):** tree of bases, groups, units with mini icons from the atlas, status text in state colours, fuel bar.
- **Bottom dock:** tabs Alerts · Approvals · Intel log · Event log · Operations. Event lines start with mono time `T+0:17:15` and a coloured category chip.
- **Operation planner:** phases as rows on a timeline; bars in `side.own`, active phase outlined amber, failed `state.bad`.

## 9. Chrome

| Element | Style |
|---|---|
| OPEN 3D TACTICAL VIEW / OPEN 2D MAP | Top right, `chrome.bg`, JetBrains Mono 12 px uppercase, letter-spacing 0.15em, `text.secondary` |
| Toasts | Bottom centre, `toast.bg`, radius 6 px, 14 px text (`Simulation paused`, `Auto-slow: missile raid detected`) |
| Zoom buttons | 32 px squares, `chrome.bg`, white `+` / `−` |
| Scale bar | White box, dark text, above the zoom buttons |

## 10. Typography

All fonts bundled with the app (offline), all free (OFL).

| Role | Font |
|---|---|
| Map labels | Noto Sans Regular / Italic / Medium (MapLibre glyphs from the Protomaps assets) |
| UI text | Noto Sans, 13–15 px |
| Commands, clock, data, tags | JetBrains Mono |

## 11. 3D view

Same tokens, atlas and label grammar. Trails and trajectories at true altitude; units get a thin vertical stalk to the ground; satellites show orbit arcs in `sat`. The 3D button reads `OPEN 2D MAP` there.

## 12. Do not

- No emoji, gradients or glow except the targeted halo.
- No default MapLibre markers, popups or Cesium widgets (timeline, animation, geocoder, base-layer picker).
- No light map theme; no colour that is not a token.
- No meaning carried by colour alone: shape, dash pattern and labels must also tell own from enemy, offensive from interceptor.

## 13. Checking a UI change

Before finishing any UI task, run the app, open the same kind of view as the closest reference screenshot (e.g. the Baltic around Kaliningrad at zoom 7 for 01, 03, 04), take a screenshot, and list the differences in the phase report. Accept differences only where this guide deliberately changes the reference (§1 for 02, §7 for emoji).
