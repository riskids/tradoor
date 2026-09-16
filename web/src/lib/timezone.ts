/**
 * The time zone every written time is in. Local by default — each device its
 * own — or UTC, or any named zone, chosen in Settings and kept with the
 * workspace. Candles stay in UTC underneath; only what is read changes.
 */
import { doc } from '../workspace/store';

export const TIME_ZONE_KEY = 'ui.timeZone';
export const LOCAL_ZONE = 'local';

/** `local`, `UTC` or an IANA name such as `Asia/Jakarta`. */
export const timeZone = (): string => doc.theme[TIME_ZONE_KEY] ?? LOCAL_ZONE;

/** The device's own zone, by name. */
export const deviceZone = (): string => Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';

export function isTimeZone(zone: string): boolean {
  if (zone === LOCAL_ZONE) return true;
  try {
    new Intl.DateTimeFormat('en-US', { timeZone: zone });
    return true;
  } catch {
    return false;
  }
}

const formatters = new Map<string, Intl.DateTimeFormat>();

function parts(zone: string, ms: number): Record<string, string> {
  let f = formatters.get(zone);
  if (!f) {
    f = new Intl.DateTimeFormat('en-US', {
      timeZone: zone === LOCAL_ZONE || !isTimeZone(zone) ? undefined : zone,
      hourCycle: 'h23',
      year: 'numeric',
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    });
    formatters.set(zone, f);
  }
  const out: Record<string, string> = {};
  for (const p of f.formatToParts(ms)) out[p.type] = p.value;
  return out;
}

/** Minutes east of UTC in `zone` at `ms`; daylight saving included. */
export function offsetMinutes(zone: string, ms = Date.now()): number {
  const p = parts(zone, ms);
  const wall = Date.UTC(Number(p.year), Number(p.month) - 1, Number(p.day), Number(p.hour), Number(p.minute), Number(p.second));
  return Math.round((wall - Math.floor(ms / 1000) * 1000) / 60_000);
}

/** `UTC`, `UTC+7`, `UTC-3:30`. */
export function offsetLabel(minutes: number): string {
  if (minutes === 0) return 'UTC';
  const a = Math.abs(minutes);
  const m = a % 60;
  return `UTC${minutes > 0 ? '+' : '-'}${Math.floor(a / 60)}${m ? `:${String(m).padStart(2, '0')}` : ''}`;
}

export const zoneLabel = (zone = timeZone()): string => offsetLabel(offsetMinutes(zone));

/** `14:03:22`. */
export function formatClock(ms: number, zone = timeZone()): string {
  const p = parts(zone, ms);
  return `${p.hour}:${p.minute}:${p.second}`;
}

/** `14:03`. */
export function formatHm(ms: number, zone = timeZone()): string {
  const p = parts(zone, ms);
  return `${p.hour}:${p.minute}`;
}

/** `2026-09-16 14:03 UTC+7`. */
export function formatDateTime(ms: number, zone = timeZone()): string {
  const p = parts(zone, ms);
  return `${p.year}-${p.month}-${p.day} ${p.hour}:${p.minute} ${zoneLabel(zone)}`;
}
