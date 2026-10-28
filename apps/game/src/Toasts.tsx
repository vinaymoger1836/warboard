// Toasts (doc 10 §9): bottom centre, plain text.
import { useCallback, useRef, useState } from 'react';

const TOAST_MS = 4_000;

export interface Toast {
  id: number;
  text: string;
}

export function useToasts(): { toasts: Toast[]; show: (text: string) => void } {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const next = useRef(1);
  const show = useCallback((text: string) => {
    const id = next.current++;
    setToasts((t) => [...t.filter((x) => x.text !== text), { id, text }].slice(-3));
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), TOAST_MS);
  }, []);
  return { toasts, show };
}

export function Toasts({ toasts, sticky }: { toasts: Toast[]; sticky: string[] }) {
  if (toasts.length === 0 && sticky.length === 0) return null;
  return (
    <div className="toasts" role="status" aria-live="polite">
      {sticky.map((t) => (
        <div key={`sticky-${t}`} className="toast">
          {t}
        </div>
      ))}
      {toasts.map((t) => (
        <div key={t.id} className="toast">
          {t.text}
        </div>
      ))}
    </div>
  );
}
