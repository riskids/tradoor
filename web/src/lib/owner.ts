/**
 * This browser's id: 128 random bits made on first use and kept, sent with
 * every request for drawings and layouts, so they are this browser's alone —
 * another browser, or a private window, starts empty. Knowing it is what makes
 * them yours, so it moves to another device by pasting it there (Settings).
 */
import { readPref, writePref } from './prefs';

export const OWNER_KEY = 'owner';
export const OWNER_PATTERN = /^[A-Za-z0-9-]{16,64}$/;

let cached: string | null = null;

export function ownerId(): string {
  if (cached) return cached;
  const stored = readPref(OWNER_KEY);
  cached = stored && OWNER_PATTERN.test(stored) ? stored : Array.from(crypto.getRandomValues(new Uint8Array(16)), (b) => b.toString(16).padStart(2, '0')).join('');
  if (cached !== stored) writePref(OWNER_KEY, cached);
  return cached;
}
