// Our weapons in flight on the 2D map (doc 10 §5, references 02 and 04): amber missile
// glyphs with a 1.5 px amber trail and `M-n` tags, cyan dashed interceptor trails with a
// cyan missile glyph and `INT-n` tags, salvo labels at each salvo's centroid, interceptor group
// labels at each shooter's interceptors, and dashed amber shooter→target lines. The data comes
// from `scene/weapons.ts`, shared with the 3D view. Other sides' weapons are contacts of our
// picture and drawn with the contacts.
import type { Layer } from '@deck.gl/core';
import { PathStyleExtension, type PathStyleExtensionProps } from '@deck.gl/extensions';
import { IconLayer, PathLayer, TextLayer } from '@deck.gl/layers';
import type { SideView, StringTable } from '@warboard/protocol';
import { missileDataUrl, rgba, type TokenName } from '@warboard/symbols';
import type { LonLat } from '../../scene/geo';
import { buildWeapons, type DrawnWeapon, type Trail, TrailStore, type WeaponLabel, weaponTags } from '../../scene/weapons';

/** Per-weapon tags from this zoom (doc 10 §5). */
const TAG_ZOOM = 7;
const MISSILE_PX = 20;
/** Tags closer than this on screen share one label, `M-8 M-6` (reference 04). */
const TAG_MERGE_PX = 28;

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
    const { weapons, trails, lines, labels } = buildWeapons(view, strings, this.store, dtS, targetAt);
    const missiles = weapons.filter((d) => !d.interceptor);
    const heads = weapons.filter((d) => d.interceptor);
    // Web Mercator metres per pixel at this zoom and latitude.
    const lat = weapons[0]?.position[1] ?? 0;
    const merge = TAG_MERGE_PX * ((156_543 * Math.cos((lat * Math.PI) / 180)) / 2 ** zoom);
    const tags: WeaponLabel[] = zoom >= TAG_ZOOM ? weaponTags(weapons, merge) : [];

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
      new IconLayer<DrawnWeapon>({
        id: 'missile-icons',
        data: missiles,
        getIcon: () => MISSILE_ICON,
        getPosition: (d) => d.position,
        getAngle: (d) => -d.heading,
        getSize: MISSILE_PX,
        sizeUnits: 'pixels',
      }),
      // Interceptors use the missile glyph in cyan (doc 10 §5); the dashed trail and the
      // `INT-n` tags still tell them from offensive missiles without colour.
      new IconLayer<DrawnWeapon>({
        id: 'interceptor-icons',
        data: heads,
        getIcon: () => INTERCEPTOR_ICON,
        getPosition: (d) => d.position,
        getAngle: (d) => -d.heading,
        getSize: MISSILE_PX * 0.85,
        sizeUnits: 'pixels',
      }),
      new TextLayer<WeaponLabel>({
        id: 'weapon-tags',
        data: tags,
        getPosition: (d) => d.position,
        getText: (d) => d.text,
        getSize: 11,
        getColor: c('label.unit'),
        getPixelOffset: [9, 0],
        getTextAnchor: 'start',
        getAlignmentBaseline: 'center',
        fontFamily: 'JetBrains Mono',
        fontSettings: SDF,
        outlineWidth: 0.3,
        outlineColor: c('map.label.halo'),
        characterSet: 'auto',
      }),
      new TextLayer<WeaponLabel>({
        id: 'salvo-labels',
        data: labels,
        getPosition: (d) => d.position,
        getText: (d) => d.text,
        getSize: 11,
        getColor: c('label.unit'),
        getPixelOffset: [0, 14],
        getAlignmentBaseline: 'top',
        fontFamily: 'Noto Sans',
        fontWeight: 'italic',
        fontSettings: SDF,
        outlineWidth: 0.3,
        outlineColor: c('map.label.halo'),
        characterSet: 'auto',
      }),
    ];
  }
}
