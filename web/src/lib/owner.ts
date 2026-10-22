/**
 * Who this browser is to the server. Before signing in: an id of its own, 128
 * random bits made on first use and kept, sent with every request, so its
 * drawings and layouts are its alone — another browser, or a private window,
 * starts empty; knowing the id is what makes them its, and pasting it in
 * Settings carries them by hand. Signed in with Telegram: a session of the
 * account instead, a token of this browser's own that signing out ends. An
 * account's id alone opens nothing (bins/server session).
 */
import { readPref, writePref } from './prefs';

export const OWNER_KEY = 'owner';
export const OWNER_PATTERN = /^[A-Za-z0-9-]{16,64}$/;
const SESSION_KEY = 'session';
const SESSION_PATTERN = /^[0-9a-f]{64}$/;
/** A word for the next load, after a sign-out it did not ask for. */
const NOTICE_KEY = 'signedOutNotice';

let cached: string | null = null;

export function ownerId(): string {
  if (cached) return cached;
  const stored = readPref(OWNER_KEY);
  cached = stored && OWNER_PATTERN.test(stored) ? stored : freshOwnerId();
  if (cached !== stored) writePref(OWNER_KEY, cached);
  return cached;
}

export function freshOwnerId(): string {
  return Array.from(crypto.getRandomValues(new Uint8Array(16)), (b) => b.toString(16).padStart(2, '0')).join('');
}

/** Take another id from the next load on — the page reads it once — and load again. */
export function switchOwner(id: string): void {
  writePref(OWNER_KEY, id);
  location.reload();
}

/** This browser's session of its account, once signed in. */
export function session(): string | null {
  const s = readPref(SESSION_KEY);
  return s && SESSION_PATTERN.test(s) ? s : null;
}

/** The header the server knows this browser by. */
export function authHeaders(): Record<string, string> {
  const s = session();
  return s ? { 'x-tradoor-session': s } : { 'x-tradoor-owner': ownerId() };
}

/** Whose this browser's copy of its layout is: the session's account, or the browser's own. */
export function identity(): string {
  const s = session();
  return s ? `session:${s.slice(0, 16)}` : ownerId();
}

/** Signed in: the session is this browser's from the next load on. */
export function holdSession(token: string): void {
  writePref(SESSION_KEY, token);
  location.reload();
}

let leaving = false;

/**
 * Signed out — by asking, from another browser, or because the server no
 * longer takes what this one holds: the session is dropped and the browser
 * starts again as a new one, empty until it signs in. Many requests may say so
 * at once; the first one does it.
 */
export function startOver(notice?: string): void {
  if (leaving) return;
  leaving = true;
  writePref(SESSION_KEY, '');
  writePref(OWNER_KEY, freshOwnerId());
  if (notice) writePref(NOTICE_KEY, notice);
  location.reload();
}

/** What the last load left to say about being signed out, said once. */
export function takeNotice(): string | null {
  const n = readPref(NOTICE_KEY);
  if (n) writePref(NOTICE_KEY, '');
  return n || null;
}

/** The server's answer when what this browser holds no longer opens anything. */
export const SIGNED_OUT = 'Signed out of this browser. Sign in with Telegram in Settings to open your account here again.';
