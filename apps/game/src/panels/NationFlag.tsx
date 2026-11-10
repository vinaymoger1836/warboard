// A nation's generated flag (doc 04 §3.0, doc 10 §8): its layout and two or three colours from
// content. Flags are nation data, not UI chrome, so their colours come from the nation entry.
import type { FlagPattern } from '@warboard/protocol';

export interface FlagSpec {
  flag_pattern: FlagPattern;
  flag_colours: string[];
}

export function NationFlag({ nation, height = 16, title }: { nation: FlagSpec | undefined; height?: number; title?: string | undefined }) {
  const w = 30;
  const h = 20;
  const [a = '#586675', b = a, c] = nation?.flag_colours ?? [];
  let body;
  switch (nation?.flag_pattern) {
    case 'nordic_cross':
      body = (
        <>
          <rect width={w} height={h} fill={a} />
          <rect x={8} width={4} height={h} fill={b} />
          <rect y={8} width={w} height={4} fill={b} />
        </>
      );
      break;
    case 'horizontal':
      body = c ? (
        <>
          <rect width={w} height={h} fill={a} />
          <rect y={h / 3} width={w} height={h / 3} fill={b} />
          <rect y={(2 * h) / 3} width={w} height={h / 3} fill={c} />
        </>
      ) : (
        <>
          <rect width={w} height={h} fill={a} />
          <rect y={h / 2} width={w} height={h / 2} fill={b} />
        </>
      );
      break;
    case 'vertical':
      body = c ? (
        <>
          <rect width={w} height={h} fill={a} />
          <rect x={w / 3} width={w / 3} height={h} fill={b} />
          <rect x={(2 * w) / 3} width={w / 3} height={h} fill={c} />
        </>
      ) : (
        <>
          <rect width={w} height={h} fill={a} />
          <rect x={w / 2} width={w / 2} height={h} fill={b} />
        </>
      );
      break;
    case 'diagonal':
      body = (
        <>
          <rect width={w} height={h} fill={a} />
          <polygon points={`${w},0 ${w},${h} 0,${h}`} fill={b} />
        </>
      );
      break;
    case 'disc':
      body = (
        <>
          <rect width={w} height={h} fill={a} />
          <circle cx={w / 2} cy={h / 2} r={h / 3.2} fill={b} />
        </>
      );
      break;
    case 'chevron':
      body = (
        <>
          <rect width={w} height={h} fill={a} />
          <polygon points={`0,0 ${w * 0.45},${h / 2} 0,${h}`} fill={b} />
        </>
      );
      break;
    default:
      body = <rect width={w} height={h} fill={a} />;
  }
  return (
    <svg
      className="nation-flag"
      viewBox={`0 0 ${w} ${h}`}
      width={(height * w) / h}
      height={height}
      role={title ? 'img' : undefined}
      aria-label={title}
      aria-hidden={title ? undefined : true}
    >
      {body}
    </svg>
  );
}
