// Live check results: errors stop the pack build, band warnings do not (ADR-041).
import { idOfFile, set, useStore } from '../store';
import type { Problem } from '../wasm';

export function Problems() {
  const report = useStore((s) => s.report);
  const files = useStore((s) => s.files);
  const open = (p: Problem) => {
    const text = files[p.file.replace(/^content\//, '')];
    const id = text ? idOfFile(text) : null;
    if (id) set({ view: 'browse', selected: id });
  };
  if (!report) return <p className="hint page">Not checked yet.</p>;
  const rows = (list: Problem[], cls: string) =>
    list.map((p, i) => (
      <tr key={`${cls}${i}`} className="click" onClick={() => open(p)}>
        <td>
          <span className={`pill ${cls}`}>{cls === 'bad' ? 'error' : 'band'}</span>
        </td>
        <td className="mono">{p.file}</td>
        <td className="mono">{p.path}</td>
        <td>{p.message}</td>
      </tr>
    ));
  return (
    <div className="page">
      <p className={report.ok ? 'ok' : 'bad'}>
        {report.ok
          ? `${report.entries.length} entries pass the check (same rules as wb content check: references, units, bands, denylist).`
          : `${report.errors.length} error(s): the pack cannot be built until they are fixed.`}{' '}
        {report.warnings.length > 0 && `${report.warnings.length} value(s) outside their archetype band: say why in design.note or move them inside.`}
      </p>
      <table className="grid wide">
        <tbody>
          {rows(report.errors, 'bad')}
          {rows(report.warnings, 'warn')}
        </tbody>
      </table>
    </div>
  );
}
