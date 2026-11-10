// Map labels on the globe (doc 10 §3, doc 11 §2.4): the war's fictional places, with the same
// kinds, tokens and zoom thresholds as the 2D style (`map2d/style.ts`): seas, gulfs and
// straits in italic sea blue; islands and regions from z5; capitals from z3 and cities from z5
// with a dot; nations in capitals up to z9. Zoom thresholds become camera distances
// through `scene/lod.ts`.
import { Cartesian2, Cartesian3, Color, DistanceDisplayCondition, Math as CesiumMath, NearFarScalar } from '@cesium/core';
import { type CesiumWidget, HorizontalOrigin, LabelCollection, LabelStyle, PointPrimitiveCollection, VerticalOrigin } from '@cesium/engine';
import type { PlaceLabel } from '../engine/messages';
import { heightForZoom } from '../scene/lod';
import { colour } from './globe';

/** Camera distance of a 2D zoom at a latitude (screen of `px` CSS px tall). */
const dist = (zoom: number, lat: number, px: number) => heightForZoom(zoom, lat, CesiumMath.toRadians(60), px);

export function addPlaces(widget: CesiumWidget, places: PlaceLabel[]): () => void {
  const scene = widget.scene;
  const px = scene.canvas.clientHeight || 800;
  const labels = scene.primitives.add(new LabelCollection({ scene }));
  const dots = scene.primitives.add(new PointPrimitiveCollection());
  const halo = colour('map.label.halo');
  for (const p of places) {
    const position = Cartesian3.fromDegrees(p.lon, p.lat, 0);
    const near = (z: number) => dist(z, p.lat, px);
    let text = p.name;
    let font = 'italic 12px "Noto Sans"';
    let fill: Color = colour('map.label.sea');
    let show: DistanceDisplayCondition;
    let dot = false;
    switch (p.kind) {
      case 'sea':
        text = p.name.toUpperCase();
        font = 'italic 13px "Noto Sans"';
        show = new DistanceDisplayCondition(0, near(3));
        break;
      case 'gulf':
      case 'strait':
        text = p.name.toUpperCase();
        font = 'italic 11px "Noto Sans"';
        show = new DistanceDisplayCondition(0, near(p.kind === 'strait' ? 5.5 : 4.5));
        break;
      case 'islands':
      case 'region':
        show = new DistanceDisplayCondition(0, near(5));
        break;
      case 'capital':
      case 'city':
        fill = colour('map.label.city');
        font = 'italic 13px "Noto Sans"';
        show = new DistanceDisplayCondition(0, near(p.kind === 'capital' ? 3 : 5));
        dot = true;
        break;
      case 'nation':
        text = p.name.toUpperCase();
        font = '600 13px "Noto Sans"';
        fill = colour('map.label.country');
        show = new DistanceDisplayCondition(near(9), Number.POSITIVE_INFINITY);
        break;
      default:
        show = new DistanceDisplayCondition(0, near(5));
    }
    labels.add({
      position,
      text,
      font,
      fillColor: fill,
      outlineColor: halo,
      outlineWidth: 3,
      style: LabelStyle.FILL_AND_OUTLINE,
      horizontalOrigin: dot ? HorizontalOrigin.LEFT : HorizontalOrigin.CENTER,
      verticalOrigin: dot ? VerticalOrigin.TOP : VerticalOrigin.CENTER,
      pixelOffset: dot ? new Cartesian2(6, 4) : Cartesian2.ZERO,
      distanceDisplayCondition: show,
      translucencyByDistance: new NearFarScalar(1.5e7, 1, 2.5e7, 0),
    });
    if (dot)
      dots.add({
        position,
        pixelSize: p.kind === 'capital' ? 5 : 4,
        color: colour('map.city.dot'),
        outlineColor: colour('map.label.halo', 0.8),
        outlineWidth: 1,
        distanceDisplayCondition: show,
      });
  }
  return () => {
    if (widget.isDestroyed()) return;
    scene.primitives.remove(labels);
    scene.primitives.remove(dots);
  };
}
