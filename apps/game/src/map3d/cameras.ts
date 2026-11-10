// 3D camera modes (doc 06 §5, ADR-047): free (Cesium's own controller), follow the selection,
// watch an engagement (the weapon's shooter and target framed together) and inspect one object
// close up (orbit around it, its class model always drawn). Targets are looked up every frame
// in what the renderer drew, so a camera only ever sees the side picture.
import { Cartesian3, Ellipsoid, FixedFrameTransforms, HeadingPitchRange, Math as CesiumMath, Matrix4 } from '@cesium/core';
import type { CesiumWidget } from '@cesium/engine';
import { modelFor } from '@warboard/symbols';
import { findUnit } from '../scene/overlays';
import type { Cam3d, CamTarget } from '../store';
import type { EngineBridge } from '../bridge';
import { heightForZoom } from '../scene/lod';
import type { LonLat } from '../scene/geo';
import type { Renderer3d } from './Renderer3d';

/** Inspect distance limits, multiples of the model's length. */
const INSPECT_MIN = 1.4;
const INSPECT_MAX = 40;
const INSPECT_START = 4.5;
/** Follow distance limits, m. */
const FOLLOW_MIN_M = 2_000;
const FOLLOW_MAX_M = 400_000;
/** Look-down pitch of a fresh follow / watch camera. */
const FOLLOW_PITCH = CesiumMath.toRadians(-35);
const WATCH_PITCH = CesiumMath.toRadians(-28);

export interface TargetState {
  position: LonLat;
  alt: number;
  heading: number;
  speed: number;
  symbol: number;
}

/** Where a camera target is this frame, or null if it is no longer drawn. */
export function targetState(r: Renderer3d, t: CamTarget): TargetState | null {
  if (t.kind === 'unit') {
    const u = r.unit(t.id);
    if (u) return { position: u.position, alt: u.alt, heading: u.heading, speed: u.speed, symbol: u.symbol };
    const s = r.last?.scene.satellites.find((d) => d.id === t.id);
    return s ? { position: s.position, alt: s.alt, heading: 0, speed: 0, symbol: s.symbol } : null;
  }
  if (t.kind === 'contact') {
    const c = r.contact(t.id);
    return c ? { position: c.position, alt: c.alt, heading: c.heading, speed: c.speed, symbol: c.symbol } : null;
  }
  const w = r.weapon(t.id);
  return w ? { position: w.position, alt: w.alt, heading: w.heading, speed: w.speed, symbol: 120 } : null;
}

/** The renderer key of a target (`u:1`, `c:2`, `w:3`; satellites `s:`). */
export function targetKey(r: Renderer3d, t: CamTarget): string {
  if (t.kind === 'unit' && !r.unit(t.id) && r.last?.scene.satellites.some((d) => d.id === t.id)) return `s:${t.id}`;
  return `${t.kind[0]}:${t.id}`;
}

export class Cameras {
  private mode: Cam3d['mode'] = 'free';
  private target: CamTarget | null = null;
  /** The watched weapon's last frame, so a camera can hold while the weapon is resolved. */
  private watchRange = 0;
  /** A mode asked for before the renderer drew its first frame (opening 3D in inspect). */
  private pending: Cam3d | null = null;
  private readonly defaults: { min: number; max: number };

  constructor(
    private readonly widget: CesiumWidget,
    private readonly renderer: Renderer3d,
    private readonly bridge: EngineBridge,
    /** Called when a mode cannot continue (target gone, no longer identified). */
    private readonly onLost: (why: string) => void,
  ) {
    const c = widget.scene.screenSpaceCameraController;
    this.defaults = { min: c.minimumZoomDistance, max: c.maximumZoomDistance };
  }

  /** Switches mode; called when the store's `cam3d` changes. */
  set(cam: Cam3d): void {
    if (!this.renderer.last) {
      this.pending = cam;
      return;
    }
    this.pending = null;
    const same = cam.mode === this.mode && cam.target?.kind === this.target?.kind && cam.target?.id === this.target?.id;
    if (same) return;
    this.release();
    this.mode = cam.mode;
    this.target = cam.target;
    this.renderer.forceModel = null;
    if (cam.mode === 'free' || !cam.target) {
      this.mode = 'free';
      return;
    }
    const t = targetState(this.renderer, cam.target);
    if (!t) {
      this.onLost('That object is not in our picture any more');
      return;
    }
    const camera = this.widget.scene.camera;
    const at = Cartesian3.fromDegrees(t.position[0], t.position[1], t.alt);
    if (cam.mode === 'inspect') {
      const len = modelFor(t.symbol)?.lengthM ?? 20;
      this.renderer.forceModel = targetKey(this.renderer, cam.target);
      const c = this.widget.scene.screenSpaceCameraController;
      c.minimumZoomDistance = len * INSPECT_MIN;
      c.maximumZoomDistance = len * INSPECT_MAX;
      camera.lookAtTransform(
        FixedFrameTransforms.eastNorthUpToFixedFrame(at),
        new HeadingPitchRange(CesiumMath.toRadians(t.heading + 140), CesiumMath.toRadians(-18), len * INSPECT_START),
      );
    } else if (cam.mode === 'follow') {
      const range = Math.min(FOLLOW_MAX_M, Math.max(FOLLOW_MIN_M, Cartesian3.distance(camera.positionWC, at) * 0.6));
      camera.lookAtTransform(FixedFrameTransforms.eastNorthUpToFixedFrame(at), new HeadingPitchRange(camera.heading, FOLLOW_PITCH, range));
    } else {
      this.watchRange = 0;
    }
  }

  /** Leaves any locked camera; free flight from where it is. */
  private release(): void {
    const camera = this.widget.scene.camera;
    if (this.mode !== 'free') camera.lookAtTransform(Matrix4.IDENTITY);
    const c = this.widget.scene.screenSpaceCameraController;
    c.minimumZoomDistance = this.defaults.min;
    c.maximumZoomDistance = this.defaults.max;
  }

  /** Every frame, after the renderer: keeps the camera on its target. */
  update(): void {
    if (this.pending && this.renderer.last) this.set(this.pending);
    if (this.mode === 'free' || !this.target) return;
    const camera = this.widget.scene.camera;
    if (this.mode === 'watch') {
      this.watch();
      return;
    }
    const t = targetState(this.renderer, this.target);
    if (!t) {
      this.onLost(this.mode === 'inspect' ? 'The inspected object is gone; back to free camera' : 'The followed object is gone');
      return;
    }
    if (this.target.kind === 'contact' && this.mode === 'inspect' && !this.renderer.contact(this.target.id)?.identified) {
      this.onLost('The contact is no longer identified; back to free camera');
      return;
    }
    // Keep the camera's offset in the target's local frame (the player orbits and zooms it).
    const offset = Cartesian3.clone(camera.position);
    camera.lookAtTransform(FixedFrameTransforms.eastNorthUpToFixedFrame(Cartesian3.fromDegrees(t.position[0], t.position[1], t.alt)), offset);
  }

  /** Frames the watched weapon's shooter and target (or the weapon and its shooter). */
  private watch(): void {
    const target = this.target;
    if (!target) return;
    const w = this.renderer.weapon(target.id);
    if (!w) {
      this.onLost('The engagement is over; back to free camera');
      return;
    }
    const view = this.bridge.frame?.view;
    const shooter = findUnit(view, w.shooter);
    const aim = w.target ? (this.renderer.contact(w.target)?.position ?? null) : null;
    const a = Cartesian3.fromDegrees(w.position[0], w.position[1], w.alt);
    const pts = [a, ...(shooter ? [Cartesian3.fromDegrees(shooter[0], shooter[1], 0)] : []), ...(aim ? [Cartesian3.fromDegrees(aim[0], aim[1], 0)] : [])];
    const mid = pts.reduce((s, p) => Cartesian3.add(s, p, s), new Cartesian3());
    Cartesian3.divideByScalar(mid, pts.length, mid);
    let span = 0;
    for (const p of pts) span = Math.max(span, Cartesian3.distance(p, mid));
    const want = Math.max(8_000, span * 2.4);
    this.watchRange = this.watchRange ? this.watchRange + (want - this.watchRange) * 0.08 : want;
    const ground = Ellipsoid.WGS84.scaleToGeodeticSurface(mid) ?? mid;
    // Look across the line of fire, from its left.
    const end = aim ?? shooter ?? w.position;
    const bearing = Math.atan2(end[0] - w.position[0], end[1] - w.position[1]);
    const camera = this.widget.scene.camera;
    camera.lookAt(ground, new HeadingPitchRange(bearing - Math.PI / 2, WATCH_PITCH, this.watchRange));
  }

  /** Free camera looking straight down at a point from the height of a 2D zoom. */
  static viewFrom(widget: CesiumWidget, centre: LonLat, zoom: number): void {
    const camera = widget.scene.camera;
    const canvas = widget.scene.canvas;
    const fovy = (camera.frustum as { fovy?: number }).fovy ?? CesiumMath.toRadians(60);
    const h = heightForZoom(zoom, centre[1], fovy, canvas.clientHeight || 800);
    camera.lookAtTransform(Matrix4.IDENTITY);
    camera.setView({
      destination: Cartesian3.fromDegrees(centre[0], centre[1], h),
      orientation: { heading: 0, pitch: -CesiumMath.PI_OVER_TWO, roll: 0 },
    });
  }

  /** Screen-centre ground point and equivalent zoom (for the 2D map and side switches). */
  current(): { center: LonLat; zoom: number } | null {
    const { zoom, centre } = this.renderer.zoom();
    return centre ? { center: centre, zoom } : null;
  }

  dispose(): void {
    this.release();
  }
}
