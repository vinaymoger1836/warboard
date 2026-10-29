// Map chrome (doc 10 §3, §9): zoom buttons and scale bar bottom right, layers button on
// the right edge, OPEN 3D TACTICAL VIEW top right, cursor readout and credits bottom left.
import type { Map as MapLibreMap } from 'maplibre-gl';
import { useEffect, useRef, useState } from 'react';
import type { EngineBridge } from '../bridge';
import { ui, useUi } from '../store';
import { TERRAIN_CLASSES } from './terrainOverlay';

const EARTH_CIRCUMFERENCE_M = 40_075_016.686;
const SCALE_MAX_PX = 110;

function niceDistance(maxMetres: number): number {
  const pow = 10 ** Math.floor(Math.log10(maxMetres));
  for (const step of [5, 2, 1]) if (step * pow <= maxMetres) return step * pow;
  return pow;
}

function ScaleBar({ map }: { map: MapLibreMap }) {
  const [scale, setScale] = useState({ px: 0, label: '' });
  useEffect(() => {
    const update = () => {
      const lat = map.getCenter().lat;
      const mPerPx = (EARTH_CIRCUMFERENCE_M * Math.cos((lat * Math.PI) / 180)) / (512 * 2 ** map.getZoom());
      const metres = niceDistance(mPerPx * SCALE_MAX_PX);
      setScale({
        px: Math.round(metres / mPerPx),
        label: metres >= 1000 ? `${(metres / 1000).toLocaleString('en-GB')} km` : `${metres} m`,
      });
    };
    update();
    map.on('move', update);
    return () => void map.off('move', update);
  }, [map]);
  return (
    <div className="scale-bar" style={{ width: scale.px }} aria-label={`Scale ${scale.label}`}>
      {scale.label}
    </div>
  );
}

/** Sensor rings of every own unit and estimated threat rings (doc 03 §10, doc 10 §6). */
function SensorToggles() {
  const all = useUi((s) => s.allSensorRings);
  const threat = useUi((s) => s.threatRings);
  return (
    <>
      <label className="toggle">
        <input type="checkbox" checked={all} onChange={(e) => ui.set({ allSensorRings: e.target.checked })} />
        Sensor rings, all units
      </label>
      <label className="toggle">
        <input type="checkbox" checked={threat} onChange={(e) => ui.set({ threatRings: e.target.checked })} />
        Threat rings (est.)
      </label>
    </>
  );
}

function LayersGlyph() {
  return (
    <svg width="18" height="18" viewBox="0 0 18 18" aria-hidden="true">
      <path d="M9 2 2 6l7 4 7-4-7-4Z" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round" />
      <path d="m2 9.5 7 4 7-4M2 12.5l7 4 7-4" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round" />
    </svg>
  );
}

function CursorReadout({ map, bridge }: { map: MapLibreMap; bridge: EngineBridge }) {
  const [text, setText] = useState('');
  const busy = useRef(false);
  useEffect(() => {
    const onMove = (e: { lngLat: { lat: number; lng: number } }) => {
      if (busy.current) return;
      busy.current = true;
      const { lat, lng } = e.lngLat;
      const lon = ((((lng + 180) % 360) + 360) % 360) - 180;
      bridge
        .request({ type: 'point', lat, lon })
        .then((v) => {
          const pos = `${Math.abs(lat).toFixed(3)}°${lat >= 0 ? 'N' : 'S'} ${Math.abs(lon).toFixed(3)}°${lon >= 0 ? 'E' : 'W'}`;
          if (v.length < 2) return setText(pos);
          const owner = v.length > 2 ? String.fromCharCode(...v.slice(2)).toUpperCase() : 'No country';
          const cls = TERRAIN_CLASSES[v[1] ?? 0] ?? 'water';
          setText(`${pos} · ${owner} · ${cls} · ${Math.round(v[0] ?? 0)} m`);
        })
        .catch(() => undefined)
        .finally(() => {
          busy.current = false;
        });
    };
    map.on('mousemove', onMove);
    return () => void map.off('mousemove', onMove);
  }, [map, bridge]);
  return text ? <div className="cursor-readout">{text}</div> : null;
}

export interface MapChromeProps {
  map: MapLibreMap;
  bridge: EngineBridge | null;
  geoReady: boolean;
  terrainOverlay: boolean;
  onTerrainOverlay: (on: boolean) => void;
  credits: string[];
  onToast: (text: string) => void;
}

export function MapChrome({ map, bridge, geoReady, terrainOverlay, onTerrainOverlay, credits, onToast }: MapChromeProps) {
  const [layersOpen, setLayersOpen] = useState(false);
  const [creditsOpen, setCreditsOpen] = useState(false);
  return (
    <>
      <button
        type="button"
        className="chrome-button open-3d"
        onClick={() => onToast('The 3D tactical view is not built yet')}
      >
        OPEN 3D TACTICAL VIEW
      </button>

      <button
        type="button"
        className="layers-button"
        aria-label="Map layers"
        aria-expanded={layersOpen}
        onClick={() => setLayersOpen((o) => !o)}
      >
        <LayersGlyph />
      </button>
      {layersOpen && (
        <div className="layers-panel" role="dialog" aria-label="Map layers">
          <SensorToggles />
          <label className="toggle">
            <input
              type="checkbox"
              checked={terrainOverlay}
              disabled={!geoReady}
              onChange={(e) => onTerrainOverlay(e.target.checked)}
            />
            Terrain classes
          </label>
          {terrainOverlay && (
            <ul className="legend">
              {TERRAIN_CLASSES.filter((c) => c !== 'water').map((c) => (
                <li key={c}>
                  <span className="swatch" style={{ background: `var(--terrain-${c})` }} />
                  {c[0]?.toUpperCase() + c.slice(1)}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}

      <div className="map-corner-br">
        <ScaleBar map={map} />
        <div className="zoom-buttons">
          <button type="button" className="zoom-button" aria-label="Zoom in" onClick={() => map.zoomIn()}>
            +
          </button>
          <button type="button" className="zoom-button" aria-label="Zoom out" onClick={() => map.zoomOut()}>
            −
          </button>
        </div>
      </div>

      <div className="map-corner-bl">
        {bridge && geoReady && <CursorReadout map={map} bridge={bridge} />}
        <button type="button" className="credits-button" onClick={() => setCreditsOpen((o) => !o)}>
          © OpenStreetMap · Credits
        </button>
      </div>
      {creditsOpen && (
        <div className="credits-panel" role="dialog" aria-label="Credits">
          <div className="panel-title">
            Credits
            <button type="button" className="close" aria-label="Close credits" onClick={() => setCreditsOpen(false)}>
              ×
            </button>
          </div>
          <ul>
            {credits.map((c) => (
              <li key={c}>{c}</li>
            ))}
            <li>Map rendering: MapLibre GL JS (BSD-3-Clause), deck.gl (MIT), PMTiles (BSD-3-Clause)</li>
            <li>Fonts: Noto Sans and JetBrains Mono (SIL Open Font License 1.1)</li>
          </ul>
        </div>
      )}
    </>
  );
}
