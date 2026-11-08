// Registry of editors with unsaved changes, so the app can warn before closing.
const dirty = new Set<string>();

export function setDirty(id: string, isDirty: boolean) {
  if (isDirty) dirty.add(id);
  else dirty.delete(id);
}

export function dirtyEditors(): string[] {
  return [...dirty];
}
