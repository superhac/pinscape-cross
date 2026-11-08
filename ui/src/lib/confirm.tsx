import { useEffect, useState } from "react";

// In-app confirmation dialog (native JS confirm() is unreliable in some webviews).

interface Req {
  title: string;
  body: string;
  ok: string;
  danger?: boolean;
  resolve: (v: boolean) => void;
}

let push: ((r: Req) => void) | null = null;

export function confirm(opts: { title: string; body: string; ok?: string; danger?: boolean }): Promise<boolean> {
  return new Promise((resolve) => {
    if (!push) return resolve(false);
    push({ ok: "OK", ...opts, resolve });
  });
}

export function ConfirmHost() {
  const [req, setReq] = useState<Req | null>(null);
  useEffect(() => {
    push = setReq;
    return () => {
      push = null;
    };
  }, []);
  useEffect(() => {
    if (!req) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        done(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
  if (!req) return null;
  const done = (v: boolean) => {
    req.resolve(v);
    setReq(null);
  };
  return (
    <div className="modal-backdrop" onClick={() => done(false)}>
      <div
        className="modal"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="confirm-title"
        aria-describedby="confirm-body"
        onClick={(e) => e.stopPropagation()}
      >
        <h2 id="confirm-title">{req.title}</h2>
        <p id="confirm-body" className="prewrap">
          {req.body}
        </p>
        <div className="modal-actions">
          <button onClick={() => done(false)}>Cancel</button>
          <button className={req.danger ? "danger" : "primary"} autoFocus onClick={() => done(true)}>
            {req.ok}
          </button>
        </div>
      </div>
    </div>
  );
}
