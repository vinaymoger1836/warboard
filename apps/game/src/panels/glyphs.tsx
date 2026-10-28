// Small SVG glyphs for panel rows (doc 10 §7: icons, not emoji). Colour comes from
// `currentColor`.

const svg = (children: React.ReactNode) => (
  <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.5">
    {children}
  </svg>
);

/** Radar / sensor horizon. */
export const SensorGlyph = () =>
  svg(
    <>
      <path d="M2 13h12" strokeLinecap="round" />
      <path d="M3 13a5 5 0 0 1 10 0" />
      <path d="M8 13l4-6" strokeLinecap="round" />
    </>,
  );

/** Max engagement range: crosshair. */
export const EngageGlyph = () =>
  svg(
    <>
      <circle cx="8" cy="8" r="5" />
      <path d="M8 1v4M8 11v4M1 8h4M11 8h4" strokeLinecap="round" />
    </>,
  );

/** Combat radius: target. */
export const RadiusGlyph = () =>
  svg(
    <>
      <circle cx="8" cy="8" r="6" />
      <circle cx="8" cy="8" r="2.5" fill="currentColor" stroke="none" />
    </>,
  );

export const CloseGlyph = () =>
  svg(<path d="M4 4l8 8M12 4l-8 8" strokeLinecap="round" />);

export const PauseGlyph = () => (
  <svg width="10" height="12" viewBox="0 0 10 12" aria-hidden="true">
    <rect x="1" y="1" width="3" height="10" fill="currentColor" />
    <rect x="6" y="1" width="3" height="10" fill="currentColor" />
  </svg>
);
