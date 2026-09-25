/**
 * The minutes whose taker flow ran hard, for the charts to mark.
 *
 * Kept for the page rather than for one chart: two charts on the same market
 * ask once between them, and a chart mounted afresh — a tab switched away from
 * and back — draws what the page already knows instead of an empty chart
 * waiting on a request.
 */
import { createStore } from 'solid-js/store';
import { api, type FlowMinute } from './api';

/** How often a market's marks are asked for again while a chart shows it. */
const POLL_MS = 15_000;

const [marks, setMarks] = createStore<Record<string, FlowMinute[]>>({});
/** What the page knows of a market's marked minutes; empty until it has asked. */
export const flowMarks = (market: string): FlowMinute[] => marks[market] ?? [];

interface Watcher {
  readers: number;
  timer: ReturnType<typeof setInterval>;
}

const watchers = new Map<string, Watcher>();

async function read(market: string): Promise<void> {
  try {
    const { minutes } = await api.flowMinutes(market);
    setMarks(market, minutes);
  } catch {
    // A server that follows no flow simply has none to mark.
  }
}

/** Follow `market`'s marks until the returned function is called. */
export function watchFlowMarks(market: string): () => void {
  const held = watchers.get(market);
  if (held) {
    held.readers += 1;
  } else {
    void read(market);
    watchers.set(market, { readers: 1, timer: setInterval(() => void read(market), POLL_MS) });
  }
  let released = false;
  return () => {
    if (released) return;
    released = true;
    const watcher = watchers.get(market);
    if (!watcher) return;
    watcher.readers -= 1;
    if (watcher.readers > 0) return;
    clearInterval(watcher.timer);
    watchers.delete(market);
  };
}
