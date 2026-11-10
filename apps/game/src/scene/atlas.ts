// One icon atlas per page (ADR-020), shared by the 2D map and the 3D view, and the label
// fonts both views draw with (loaded before the first label so no fallback font is used).
import { buildAtlas, type IconAtlas } from '@warboard/symbols';

let atlasPromise: Promise<IconAtlas> | null = null;
export const atlas = (): Promise<IconAtlas> => (atlasPromise ??= buildAtlas());

const LABEL_FONTS = ['italic 400 32px "Noto Sans"', '400 32px "Noto Sans"', '600 32px "Noto Sans"', '700 32px "Noto Sans"', '600 32px "JetBrains Mono"', '400 32px "JetBrains Mono"'];

/** The atlas, once the label fonts are loaded. */
export async function atlasAndFonts(): Promise<IconAtlas> {
  const [a] = await Promise.all([atlas(), ...LABEL_FONTS.map((f) => document.fonts.load(f))]);
  return a;
}
