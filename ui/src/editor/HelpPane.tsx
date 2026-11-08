interface Props {
  /** Page under /help, e.g. "JSONConfigRef.htm". */
  page: string;
  anchor: string | null;
  onClose: () => void;
}

/** Side panel showing the upstream HTML help (bundled under /help). */
export default function HelpPane({ page, anchor, onClose }: Props) {
  // A changing key forces the iframe to navigate even when only the anchor changes.
  const src = `/help/${page}${anchor ? `#${anchor}` : ""}`;
  return (
    <aside className="help-pane">
      <div className="help-bar">
        <strong>Reference</strong>
        <span className="muted small">{anchor ?? page}</span>
        <span className="spacer" />
        <a className="small" href="/help/JSONConfigRef.htm" target="help-frame">
          Contents
        </a>
        <button onClick={onClose} aria-label="Close help">
          ✕
        </button>
      </div>
      <iframe key={src} name="help-frame" title="Pinscape reference" src={src} />
    </aside>
  );
}
