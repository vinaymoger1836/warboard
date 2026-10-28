# Bundled fonts (doc 10 §10)

All fonts are under the SIL Open Font License 1.1 (accepted for fonts only in P2; see
`docs/versions.md`). The licence texts sit next to the files.

| Folder | What | Source (pinned) |
|---|---|---|
| `glyphs/<stack>/<range>.pbf` | MapLibre glyph ranges for Noto Sans Regular, Italic and Medium: Latin, Latin Extended, Greek, Cyrillic and punctuation (0–1279, 7680–7935, 8192–8447) | `protomaps/basemaps-assets` commit `028c18f713baecad011301ff7a69acc39bcc2ae7`, `fonts/` |
| `web/noto-sans-*.woff2` | Noto Sans for UI text (400, 500, 600, 400 italic; Latin, Latin Extended, Cyrillic) | `@fontsource/noto-sans` 5.3.0 `files/` via jsDelivr |
| `web/jetbrains-mono-*.woff2` | JetBrains Mono for commands, clock, data and tags (400, 600; Latin) | `@fontsource/jetbrains-mono` 5.3.0 `files/` via jsDelivr |

Labels in scripts outside these ranges render without glyphs; map labels use English names.
