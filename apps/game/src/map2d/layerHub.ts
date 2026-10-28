// One deck.gl overlay, several producers (terrain overlay, unit layers): each sets its own
// slot and the hub hands the overlay all slots in a fixed order (terrain under units).
import type { Layer } from '@deck.gl/core';
import type { MapboxOverlay } from '@deck.gl/mapbox';

const ORDER = ['terrain', 'units'] as const;
export type Slot = (typeof ORDER)[number];

export class LayerHub {
  private readonly slots = new Map<Slot, Layer[]>();

  constructor(private readonly overlay: MapboxOverlay) {}

  set(slot: Slot, layers: Layer[]): void {
    this.slots.set(slot, layers);
    this.overlay.setProps({ layers: ORDER.flatMap((s) => this.slots.get(s) ?? []) });
  }

  get deck(): MapboxOverlay {
    return this.overlay;
  }
}
