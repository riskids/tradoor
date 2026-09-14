/** Per-browser conveniences in localStorage. Every access is guarded: storage
 *  throws in private windows and when blocked, and a chart that will not open
 *  because it could not remember something would be a poor trade. */

const PREFIX = 'tradoor:';

export function readPref(key: string): string | null {
  try {
    return localStorage.getItem(PREFIX + key);
  } catch {
    return null;
  }
}

export function writePref(key: string, value: string): void {
  try {
    localStorage.setItem(PREFIX + key, value);
  } catch {
    // Full or blocked: the value still applies to this session.
  }
}

export function readJson(key: string): unknown {
  const raw = readPref(key);
  if (raw === null) return null;
  try {
    return JSON.parse(raw);
  } catch {
    return null;
  }
}
