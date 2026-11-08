import { ReactNode, useEffect, useId, useRef } from "react";

interface Props {
  title: string;
  onClose: () => void;
  children: ReactNode;
  /** Buttons for the bottom row (a Close button is added if omitted). */
  actions?: ReactNode;
  className?: string;
}

const FOCUSABLE = 'button:not(:disabled), [href], input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])';

/**
 * Accessible dialog: Escape closes it (only the topmost one), focus moves into
 * it and stays there (Tab wraps), and returns to where it was on close.
 */
export default function Modal({ title, onClose, children, actions, className }: Props) {
  const backdrop = useRef<HTMLDivElement>(null);
  const box = useRef<HTMLDivElement>(null);
  const titleId = useId();
  const close = useRef(onClose);
  close.current = onClose;

  useEffect(() => {
    const prev = document.activeElement as HTMLElement | null;
    const first = box.current?.querySelector<HTMLElement>("[autofocus], " + FOCUSABLE);
    (first ?? box.current)?.focus();
    const topmost = () => {
      const all = document.querySelectorAll(".modal-backdrop");
      return all[all.length - 1] === backdrop.current;
    };
    const onKey = (e: KeyboardEvent) => {
      if (!topmost()) return;
      if (e.key === "Escape") {
        e.preventDefault();
        close.current();
      } else if (e.key === "Tab" && box.current) {
        const items = [...box.current.querySelectorAll<HTMLElement>(FOCUSABLE)];
        if (!items.length) return;
        const a = items[0];
        const z = items[items.length - 1];
        if (e.shiftKey && document.activeElement === a) {
          e.preventDefault();
          z.focus();
        } else if (!e.shiftKey && document.activeElement === z) {
          e.preventDefault();
          a.focus();
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      prev?.focus?.();
    };
  }, []);

  return (
    <div className="modal-backdrop" ref={backdrop} onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div
        className={"modal " + (className ?? "")}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        ref={box}
        tabIndex={-1}
      >
        <h2 id={titleId}>{title}</h2>
        {children}
        <div className="modal-actions">{actions ?? <button onClick={onClose}>Close</button>}</div>
      </div>
    </div>
  );
}
