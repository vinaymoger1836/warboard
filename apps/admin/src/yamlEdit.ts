// Edits YAML entries in place: a form change becomes setIn/deleteIn on the parsed
// document, so comments, key order and flow style (`{ a: 1 }`) survive the round trip.
import { type Document, isMap, isSeq, parseDocument } from 'yaml';

type Path = (string | number)[];

export function toData(text: string): Record<string, unknown> {
  try {
    const v: unknown = parseDocument(text).toJS();
    return v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : {};
  } catch {
    return {};
  }
}

function plain(v: unknown): v is Record<string, unknown> {
  return !!v && typeof v === 'object' && !Array.isArray(v);
}

function same(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

/** Empty strings and undefined from form fields are not written. */
function clean(v: unknown): unknown {
  if (Array.isArray(v)) return v.map(clean);
  if (plain(v)) {
    const out: Record<string, unknown> = {};
    for (const [k, x] of Object.entries(v)) {
      if (x === undefined || x === '') continue;
      out[k] = clean(x);
    }
    return out;
  }
  return v;
}

function replace(doc: Document, path: Path, value: unknown): void {
  const old = doc.getIn(path, true);
  const node = doc.createNode(value);
  if ((isMap(old) || isSeq(old)) && (isMap(node) || isSeq(node))) {
    node.flow = old.flow ?? false;
    // A block list of flow maps (stations, basing) keeps its items flow.
    if (isSeq(old) && isSeq(node)) {
      const first = old.items[0];
      if ((isMap(first) || isSeq(first)) && first.flow) {
        for (const it of node.items) if (isMap(it) || isSeq(it)) it.flow = true;
      }
    }
  }
  doc.setIn(path, node);
}

function patch(doc: Document, path: Path, a: unknown, b: unknown): void {
  if (same(a, b)) return;
  if (b === undefined) {
    doc.deleteIn(path);
    return;
  }
  if (plain(a) && plain(b)) {
    for (const k of Object.keys(a)) if (!(k in b)) doc.deleteIn([...path, k]);
    for (const [k, v] of Object.entries(b)) patch(doc, [...path, k], a[k], v);
    return;
  }
  if (Array.isArray(a) && Array.isArray(b) && a.length === b.length) {
    b.forEach((v, i) => patch(doc, [...path, i], a[i], v));
    return;
  }
  if (path.length === 0) return;
  replace(doc, path, b);
}

/**
 * Applies only what changed between two form states (prev to next) to the document,
 * so defaults a form fills in but the file never had are not written.
 */
export function applyDelta(text: string, prev: Record<string, unknown>, next: Record<string, unknown>): string {
  const doc = parseDocument(text);
  patch(doc, [], clean(prev), clean(next));
  return doc.toString({ lineWidth: 0 });
}

/** Sets one value by path, e.g. `['platforms', 'mf-4']`; `undefined` deletes it. */
export function setIn(text: string, path: Path, value: unknown): string {
  const doc = parseDocument(text);
  if (value === undefined) doc.deleteIn(path);
  else doc.setIn(path, doc.createNode(value));
  return doc.toString({ lineWidth: 0 });
}
