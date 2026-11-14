// Credits screen (doc 06 §8, ADR-018, P12): every attribution the installed packs require, read
// from their manifests, plus the software and fonts the app is built on. One screen for the
// menu, the map's credits button and the Android app.
import type { InstalledPack, PackSource } from '../packs/types';

/** Libraries and fonts shipped in the app (docs/versions.md has the exact versions). */
const SOFTWARE: { name: string; licence: string }[] = [
  { name: 'MapLibre GL JS', licence: 'BSD-3-Clause' },
  { name: 'deck.gl and luma.gl', licence: 'MIT' },
  { name: 'PMTiles', licence: 'BSD-3-Clause' },
  { name: 'CesiumJS (3D view, self-hosted)', licence: 'Apache-2.0' },
  { name: 'React', licence: 'MIT' },
  { name: 'Capacitor (Android app)', licence: 'MIT' },
  { name: 'Noto Sans and JetBrains Mono (fonts)', licence: 'SIL Open Font License 1.1' },
];

function source(s: PackSource) {
  return (
    <li key={s.id}>
      <span className="credit-name">{s.name}</span>
      <span className="credit-meta">
        {' '}
        · {s.version} · {s.licence} · accessed {s.accessed}
      </span>
    </li>
  );
}

export function Credits({ packs, onClose }: { packs: InstalledPack[]; onClose: () => void }) {
  const attributions = [...new Set(packs.flatMap((p) => p.manifest.attribution))];
  return (
    <section className="panel credits-screen" role="dialog" aria-modal="true" aria-label="Credits">
      <h2 className="panel-header">
        Credits
        <button type="button" className="close" aria-label="Close credits" onClick={onClose}>
          ×
        </button>
      </h2>
      <div className="credits-body">
        <h3>Map and terrain data</h3>
        {packs.length === 0 ? (
          <p className="muted">No map pack is installed. Install one from MAP PACKS; its sources and attributions appear here.</p>
        ) : (
          <>
            {attributions.length > 0 && (
              <ul className="credit-list">
                {attributions.map((a) => (
                  <li key={a}>{a}</li>
                ))}
              </ul>
            )}
            {packs.map((p) => (
              <div key={p.id} className="credit-pack">
                <h4>
                  {p.manifest.name} <span className="credit-meta">pack {p.version}</span>
                </h4>
                <ul className="credit-list">{p.manifest.sources.map(source)}</ul>
              </div>
            ))}
          </>
        )}
        <h3>Software</h3>
        <ul className="credit-list">
          {SOFTWARE.map((s) => (
            <li key={s.name}>
              <span className="credit-name">{s.name}</span>
              <span className="credit-meta"> · {s.licence}</span>
            </li>
          ))}
        </ul>
        <h3>The setting</h3>
        <p className="muted">
          The nations, borders, bases, place names and weapon systems in this game are fictional. Only the terrain is real.
        </p>
      </div>
    </section>
  );
}
