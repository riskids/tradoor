/**
 * The partner exchanges' card: offered once to a browser, a minute into its
 * first visit, and not again once closed or used. The links themselves, and
 * the dialog that keeps them at hand, load only when shown (components/Referrals).
 *
 * The site's owner opens the site once with `?noreferral` on each device and
 * is never offered it there.
 */
import { createSignal } from 'solid-js';
import { readPref, writePref } from './prefs';

const KEY = 'referral';
const DELAY_MS = 60_000;

export const [referralCard, setReferralCard] = createSignal(false);

export function startReferral(): void {
  const url = new URL(location.href);
  if (url.searchParams.has('noreferral')) {
    writePref(KEY, 'off');
    url.searchParams.delete('noreferral');
    history.replaceState(history.state, '', url);
  }
  if (readPref(KEY)) return;
  setTimeout(() => setReferralCard(true), import.meta.env.DEV && url.searchParams.has('referral') ? 500 : DELAY_MS);
}

/** Closed, or a link in it used: it is not offered here again. */
export function dismissReferralCard(): void {
  setReferralCard(false);
  if (!readPref(KEY)) writePref(KEY, 'seen');
}
