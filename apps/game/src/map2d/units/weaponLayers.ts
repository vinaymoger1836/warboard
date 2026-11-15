// Our weapons in flight on the 2D map (doc 10 §5, references 02 and 04): amber missile
// glyphs with a 1.5 px amber trail and `M-n` tags, cyan dashed interceptor trails with a
// cyan missile glyph and `INT-n` tags, every weapon drawn (those launched together are fanned
// out sideways, no salvo label), and dashed amber shooter→target lines. The data comes
// from `scene/weapons.ts`, shared with the 3D view. Other sides' weapons are contacts of our
// picture and drawn with the contacts.
import type { Layer } from '@deck.gl/core';
import { PathStyleExtension, type PathStyleExtensionProps } from '@deck.gl/extensions';
import { IconLayer, PathLayer, TextLayer } from '@deck.gl/layers';
import type { SideView, StringTable } from '@warboard/protocol';
import { missileDataUrl, rgba, type TokenName } from '@warboard/symbols';
import type { LonLat } from '../../scene/geo';
import { distance } from '../../scene/geo';
import { buildWeapons, type DrawnWeapon, type Trail, TrailStore } from '../../scene/weapons';

/** Per-weapon tags from this zoom (doc 10 §5). */
const TAG_ZOOM = 7;
const MISSILE_PX = 20;
/** Weapons closer than this on screen are fanned out sideways so each one is drawn. */
const STACK_PX = 12;
/** Gap between fanned-out weapons, px; wider when each carries its `M-n` tag. */
const FAN_PX = 14;
const FAN_TAG_PX = 28;
/** Fanned-out weapons per row; a bigger raid continues in rows behind the first. */
const FAN_ROW = 6;

interface Spread {
  weapon: DrawnWeapon;
  /** Screen offset in pixels (x right, y down): display only, the weapon's true position is unchanged. */
  dx: number;
  dy: number;
}

/** Weapons launched together sit on one spot; spread them across their heading so none hides another. */
function fanOut(weapons: DrawnWeapon[], metresPerPx: number, gap: number): Spread[] {
  const clusters: DrawnWeapon[][] = [];
  for (const d of weapons) {
    const home = clusters.find((c) => distance((c[0] as DrawnWeapon).position, d.position) < STACK_PX * metresPerPx);
    if (home) home.push(d);
    else clusters.push([d]);
  }
  const out: Spread[] = [];
  for (const c of clusters) {
    const h = (((c[0] as DrawnWeapon).heading * Math.PI) / 180);
    const across = [Math.cos(h), Math.sin(h)];
    const back = [-Math.sin(h), Math.cos(h)];
    c.forEach((weapon, k) => {
      const inRow = Math.min(FAN_ROW, c.length - Math.floor(k / FAN_ROW) * FAN_ROW);
      const col = (k % FAN_ROW) - (inRow - 1) / 2;
      const row = Math.floor(k / FAN_ROW);
      out.push({
        weapon,
        dx: across[0]! * col * gap + back[0]! * row * gap,
        dy: across[1]! * col * gap + back[1]! * row * gap,
      });
    });
  }
  return out;
}

const dash = new PathStyleExtension({ dash: true });
const SDF = { sdf: true, fontSize: 64, buffer: 10, radius: 14, cutoff: 0.18 };
const c = (t: TokenName, a = 1) => rgba(t, a);
const MISSILE_ICON = { url: missileDataUrl(), width: 32, height: 32, id: 'missile', mask: false };
const INTERCEPTOR_ICON = { url: missileDataUrl('weapon.interceptor'), width: 32, height: 32, id: 'interceptor', mask: false };

export class WeaponLayers {
  private readonly store = new TrailStore();

  /** `targetAt` gives a contact's position as drawn this frame. */
  layers(
    view: SideView,
    receivedAt: number,
    strings: StringTable,
    dtS: number,
    zoom: number,
    targetAt: (contact: number) => LonLat | null,
  ): Layer[] {
    this.store.update(view, receivedAt);
    const { weapons, trails, lines } = buildWeapons(view, strings, this.store, dtS, targetAt);
    // Web Mercator metres per pixel at this zoom and latitude.
    const lat = weapons[0]?.position[1] ?? 0;
    const metresPerPx = (156_543 * Math.cos((lat * Math.PI) / 180)) / 2 ** zoom;
    const showTags = zoom >= TAG_ZOOM;
    const spread = fanOut(weapons, metresPerPx, showTags ? FAN_TAG_PX : FAN_PX);
    const missiles = spread.filter((d) => !d.weapon.interceptor);
    const heads = spread.filter((d) => d.weapon.interceptor);

    return [
      new PathLayer<LonLat[], PathStyleExtensionProps<LonLat[]>>({
        id: 'weapon-lines',
        data: lines,
        getPath: (d) => d,
        getColor: c('weapon.line'),
        getWidth: 1.5,
        widthUnits: 'pixels',
        getDashArray: [5, 4],
        dashJustified: true,
        extensions: [dash],
      }),
      new PathLayer<Trail, PathStyleExtensionProps<Trail>>({
        id: 'weapon-trails',
        data: trails.filter((t) => t.path.length > 1),
        getPath: (d) => d.path.map((p) => [p[0], p[1]] as LonLat),
        getColor: (d) => (d.interceptor ? c('weapon.interceptor') : c('weapon.offensive')),
        getWidth: (d) => (d.interceptor ? 2 : 1.5),
        widthUnits: 'pixels',
        getDashArray: (d) => (d.interceptor ? [4, 3] : [0, 0]),
        dashJustified: true,
        extensions: [dash],
      }),
      new IconLayer<Spread>({
        id: 'missile-icons',
        data: missiles,
        getIcon: () => MISSILE_ICON,
        getPosition: (d) => d.weapon.position,
        getPixelOffset: (d) => [d.dx, d.dy],
        getAngle: (d) => -d.weapon.heading,
        getSize: MISSILE_PX,
        sizeUnits: 'pixels',
      }),
      // Interceptors use the missile glyph in cyan (doc 10 §5); the dashed trail and the
      // `INT-n` tags still tell them from offensive missiles without colour.
      new IconLayer<Spread>({
        id: 'interceptor-icons',
        data: heads,
        getIcon: () => INTERCEPTOR_ICON,
        getPosition: (d) => d.weapon.position,
        getPixelOffset: (d) => [d.dx, d.dy],
        getAngle: (d) => -d.weapon.heading,
        getSize: MISSILE_PX * 0.85,
        sizeUnits: 'pixels',
      }),
      // One tag under each weapon (`M-3`, `INT-2`); no salvo label, every missile is drawn.
      new TextLayer<Spread>({
        id: 'weapon-tags',
        data: showTags ? spread : [],
        getPosition: (d) => d.weapon.position,
        getText: (d) => d.weapon.tag,
        getSize: 11,
        getColor: c('label.unit'),
        getPixelOffset: (d) => [d.dx, d.dy + MISSILE_PX * 0.6],
        getTextAnchor: 'middle',
        getAlignmentBaseline: 'top',
        fontFamily: 'JetBrains Mono',
        fontSettings: SDF,
        outlineWidth: 0.3,
        outlineColor: c('map.label.halo'),
        characterSet: 'auto',
      }),
    ];
  }
}
