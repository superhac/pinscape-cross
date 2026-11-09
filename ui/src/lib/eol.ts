/**
 * Line-ending helpers. The editor (Monaco) keeps one line ending per file, so a
 * file mixing CRLF and LF comes back from it with every line converted. Loading
 * such a file unchanged would then count as an unsaved edit.
 */

/** True if the text has both CRLF and bare LF line endings. */
export function hasMixedEol(t: string): boolean {
  const crlf = t.split("\r\n").length - 1;
  const lf = t.split("\n").length - 1 - crlf;
  return crlf > 0 && lf > 0;
}

/** Convert every line ending to the one the text uses most (CRLF on a tie, like the upstream files). */
export function normalizeEol(t: string): string {
  if (!hasMixedEol(t)) return t;
  const crlf = t.split("\r\n").length - 1;
  const lf = t.split("\n").length - 1 - crlf;
  const lfOnly = t.replace(/\r\n/g, "\n");
  return crlf >= lf ? lfOnly.replace(/\n/g, "\r\n") : lfOnly;
}
