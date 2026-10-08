/**
 * New signals told the moment they appear while the page is open: the list is
 * read every minute, and a pattern that turned, price at a line, or three
 * drives on the day's or the week's bars that was not there before is a notice
 * that opens it on the chart — and a notification of the system's own while
 * the tab is in the background, where the browser allows them. What was told
 * is kept, so a visit after a while tells what came meanwhile, once.
 */
import { api, type SignalsState } from './api';
import { readPref, writePref } from './prefs';
import { symbolOf } from './markets';
import { toast } from './toasts';
import { openPattern, signalPattern, touchPattern, type Openable } from './patterns';

const POLL_MS = 60_000;
const SEEN_KEY = 'signals-told';
/** As long as a pattern stays news: two days waiting and two since it turned. */
const KEEP_MS = 5 * 86_400_000;
/** Notices told one by one at most; past that, one says how many. */
const AT_MOST = 3;

interface News {
  key: string;
  title: string;
  body: string;
  bull: boolean;
  open: Openable;
}

function newsOf(state: SignalsState): News[] {
  const out: News[] = [];
  for (const s of state.signals) {
    const strong = s.timeframe !== '4h';
    const turned = s.turnedAt !== null;
    // Four hours' patterns once they turn; the day's and the week's from the start, the strong kind.
    if (!turned && !strong) continue;
    const way = s.side === 'bear' ? 'bearish' : 'bullish';
    out.push({
      key: `${s.id}:${turned ? 'turned' : 'seen'}`,
      title: `${strong ? '⭐ ' : ''}${symbolOf(s.market)} ${s.timeframe.toUpperCase()} — ${way} three drives ${turned ? 'turned' : 'forming'}`,
      body: `Score ${s.score}${s.deltaDivergence ? ' · Δ divergence' : ''}${s.divergence ? ' · RSI divergence' : ''} — open it on the chart`,
      bull: s.side === 'bull',
      open: signalPattern(s),
    });
  }
  for (const t of state.touches) {
    out.push({
      key: `${t.market}@${t.touchedAt}`,
      title: `${symbolOf(t.market)} 4H — price at the line, ${t.side === 'bear' ? 'bearish' : 'bullish'}`,
      body: 'A third drive as it happens, not yet confirmed — open it on the chart',
      bull: t.side === 'bull',
      open: touchPattern(t),
    });
  }
  return out;
}

function readSeen(): Record<string, number> | null {
  try {
    const raw = readPref(SEEN_KEY);
    return raw ? (JSON.parse(raw) as Record<string, number>) : null;
  } catch {
    return null;
  }
}

function tell(n: News): void {
  toast({ tone: 'alert', title: n.title, body: n.body, way: n.bull, action: () => openPattern(n.open, undefined, true) }, 30_000);
  if (typeof Notification === 'undefined' || Notification.permission !== 'granted' || !document.hidden) return;
  const note = new Notification(n.title, { body: n.body, tag: n.key });
  note.onclick = () => {
    focus();
    openPattern(n.open, undefined, true);
    note.close();
  };
}

export function watchSignals(): void {
  let seen = readSeen();
  const check = async () => {
    let state: SignalsState;
    try {
      state = await api.signals();
    } catch {
      return;
    }
    const now = Date.now();
    const all = newsOf(state);
    // The first look in a browser that has never looked only learns what is there.
    const fresh = seen ? all.filter((n) => !(n.key in seen!)) : [];
    const kept: Record<string, number> = {};
    for (const [key, at] of Object.entries(seen ?? {})) if (now - at < KEEP_MS) kept[key] = at;
    for (const n of all) kept[n.key] ??= now;
    seen = kept;
    writePref(SEEN_KEY, JSON.stringify(kept));
    if (fresh.length > AT_MOST) {
      fresh.slice(0, AT_MOST - 1).forEach(tell);
      toast({ tone: 'alert', title: `${fresh.length - AT_MOST + 1} more new signals`, body: 'In the Signal panel' }, 30_000);
    } else fresh.forEach(tell);
  };
  void check();
  setInterval(() => void check(), POLL_MS);
}
