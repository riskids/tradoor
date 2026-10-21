/**
 * The trading journal, over the workspace: every long or short position drawn
 * on a chart lands here as a trade, and price decides how it went (see
 * bins/server/src/journal.rs). Three tabs, as rsalianto's journal has them:
 * the dashboard of what the trades add up to, their history — each opened to
 * set its strategy and notes or close it by hand — and the weekly review. It
 * is kept for an account: until the browser signs in with Telegram, it shows
 * how, and nothing is written down.
 */
import { createMemo, createSignal, For, Match, onCleanup, onMount, Show, Switch } from 'solid-js';
import { Portal } from 'solid-js/web';
import { setJournalOpen } from '../app/ui';
import { AccountCard } from '../components/Account';
import { SymbolSearch } from '../components/SymbolSearch';
import { api, catalog, errorText, type AccountStatus } from '../lib/api';
import { formatPrice } from '../lib/format';
import { VenueTag } from '../lib/exchanges';
import { Icon } from '../lib/icons';
import { isMarket, symbolOf, venueLabel } from '../lib/markets';
import { focusPanel, marketOf, setPanelMarket, setPanelTimeframe, targetChart } from '../workspace/store';
import { StrategyPicker } from './strategies';
import {
  book,
  dailyPnl,
  FILL_LABEL,
  equityCurve,
  groupBy,
  inWeek,
  isClosed,
  isLive,
  journalApi,
  lastWeeks,
  localDay,
  localInput,
  money,
  plannedRR,
  pnl,
  QUESTIONS,
  rMultiple,
  rText,
  stamp,
  stats,
  STATUS_LABEL,
  weekOf,
  when,
  type AnswerKey,
  type Group,
  type Review,
  type Side,
  type Status,
  type Trade,
} from './model';

type Tab = 'dashboard' | 'history' | 'review';
const TABS: [Tab, string][] = [
  ['dashboard', 'Dashboard'],
  ['history', 'History'],
  ['review', 'Review'],
];
/** The server weighs trades against price every minute; the page asks again twice in that. */
const REFRESH_MS = 30_000;

const tone = (v: number | null): string => (v === null || v === 0 ? '' : v > 0 ? 'bull' : 'bear');
/** Fills that put coins in; the rest take them out. */
const IN = new Set(['entry', 'add']);

export function Journal() {
  const [trades, setTrades] = createSignal<Trade[]>([]);
  const [error, setError] = createSignal<string | null>(null);
  const [loaded, setLoaded] = createSignal(false);
  const [tab, setTab] = createSignal<Tab>('dashboard');
  const [selected, setSelected] = createSignal<number | null>(null);
  const [adding, setAdding] = createSignal(false);
  const [decimals, setDecimals] = createSignal(new Map<string, number>());
  const [account, setAccount] = createSignal<AccountStatus | null>(null);
  const signedIn = () => !!account()?.account;

  const load = async () => {
    if (!signedIn()) return;
    try {
      setTrades(await journalApi.trades());
      setError(null);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setLoaded(true);
    }
  };
  const readAccount = async () => {
    try {
      setAccount(await api.account());
      setError(null);
    } catch (e) {
      setError(errorText(e));
    }
    await load();
  };
  onMount(() => {
    void readAccount();
    // Asked again only while the page is in view and price still has a trade
    // to decide; back in view, at once.
    const timer = setInterval(() => {
      if (!document.hidden && trades().some((t) => t.auto && t.tracked && isLive(t))) void load();
    }, REFRESH_MS);
    const shown = () => {
      if (!document.hidden) void load();
    };
    document.addEventListener('visibilitychange', shown);
    onCleanup(() => {
      clearInterval(timer);
      document.removeEventListener('visibilitychange', shown);
    });
    // Each market's tick, so a dragged anchor's 81,234.56789 reads as the exchange quotes it.
    void catalog()
      .then((c) => setDecimals(new Map(c.products.flatMap((p) => (p.priceDecimals === null ? [] : [[p.market, p.priceDecimals] as [string, number]])))))
      .catch(() => {});
  });
  const onKey = (e: KeyboardEvent) => {
    if (e.key !== 'Escape') return;
    if (adding()) setAdding(false);
    else if (selected() !== null) setSelected(null);
    else setJournalOpen(false);
  };
  onMount(() => {
    window.addEventListener('keydown', onKey);
    onCleanup(() => window.removeEventListener('keydown', onKey));
  });

  const price = (market: string, v: number | null) => formatPrice(v, decimals().get(market));
  const replace = (t: Trade) => setTrades((list) => list.map((x) => (x.id === t.id ? t : x)));
  const current = () => trades().find((t) => t.id === selected());

  return (
    <section class="journal">
      <header class="jr-head">
        <span class="jr-title">Journal</span>
        <Show when={signedIn()}>
          <nav class="chips jr-tabs">
            <For each={TABS}>
              {([id, label]) => (
                <button classList={{ on: tab() === id }} onClick={() => setTab(id)}>
                  {label}
                </button>
              )}
            </For>
          </nav>
        </Show>
        <span class="grow" />
        <Show when={signedIn()}>
          <button class="tb" data-tip="Add a trade by hand" onClick={() => setAdding(true)}>
            <Icon name="plus" size={12} />
            <span>Trade</span>
          </button>
        </Show>
        <button class="iconbtn" aria-label="Close the journal" data-tip="Back to the charts (Esc)" onClick={() => setJournalOpen(false)}>
          <Icon name="close" size={15} />
        </button>
      </header>
      <Show when={error()}>{(e) => <div class="banner">▲ {e()}</div>}</Show>
      <div class="jr-body">
        <Show when={account()} fallback={<p class="jr-empty muted">Reading…</p>}>
          <Show when={signedIn()} fallback={<SignInFirst onSignedIn={() => void readAccount()} />}>
            <Show when={loaded() && trades().length === 0 && tab() !== 'review'}>
              <div class="jr-empty">
                <p>No trades yet.</p>
                <p class="muted">
                  Draw a Long or Short position on a chart (Alt+L, Alt+S): it lands here, and price decides how it went — open at the entry, then the target or the stop. Or
                  add one by hand.
                </p>
              </div>
            </Show>
            <Switch>
              <Match when={tab() === 'dashboard' && trades().length > 0}>
                <Dashboard trades={trades()} />
              </Match>
              <Match when={tab() === 'history' && trades().length > 0}>
                <History trades={trades()} price={price} onSelect={setSelected} />
              </Match>
              <Match when={tab() === 'review'}>
                <ReviewTab trades={trades()} />
              </Match>
            </Switch>
          </Show>
        </Show>
      </div>
      <Show when={current()}>
        {(t) => (
          <Editor
            trade={t()}
            price={price}
            onClose={() => setSelected(null)}
            onChange={replace}
            onRemove={(id) => {
              setTrades((list) => list.filter((x) => x.id !== id));
              setSelected(null);
            }}
          />
        )}
      </Show>
      <Show when={adding()}>
        <AddTrade
          onClose={() => setAdding(false)}
          onAdd={(t) => {
            setTrades((list) => [t, ...list]);
            setAdding(false);
          }}
        />
      </Show>
    </section>
  );
}

/** Before the journal opens, signing in: it is kept for a Telegram account. */
function SignInFirst(props: { onSignedIn(): void }) {
  return (
    <div class="jr-gate">
      <h2>Sign in with Telegram to keep a journal</h2>
      <p class="muted">
        Once you are signed in, every Long or Short position you draw on a chart lands here, and price decides how it went — open at the entry, then the target or the stop.
        The journal is kept with your account, so it follows you to any device you sign in on.
      </p>
      <AccountCard onSignedIn={props.onSignedIn} />
    </div>
  );
}

// ---------------------------------------------------------------- dashboard

const PERIODS: [string, string][] = [
  ['all', 'All'],
  ['7', '7D'],
  ['30', '30D'],
  ['90', '90D'],
];

function Dashboard(props: { trades: Trade[] }) {
  const [period, setPeriod] = createSignal('all');
  const list = createMemo(() => {
    const p = period();
    if (p === 'all') return props.trades;
    const from = Date.now() - Number(p) * 86_400_000;
    return props.trades.filter((t) => when(t) >= from);
  });
  const s = createMemo(() => stats(list()));
  return (
    <div class="jr-dash">
      <div class="chips jr-periods">
        <For each={PERIODS}>
          {([id, label]) => (
            <button classList={{ on: period() === id }} onClick={() => setPeriod(id)}>
              {label}
            </button>
          )}
        </For>
      </div>
      <div class="jr-cards">
        <Card label="Net P&L" value={money(s().totalPnl)} tone={tone(s().totalPnl)} sub={`${money(s().grossProfit)} won · ${money(-s().grossLoss)} lost`} />
        <Card label="Win rate" value={`${s().winRate.toFixed(1)}%`} sub={`${s().wins} won · ${s().losses} lost · ${s().breakEven} even`} />
        <Card label="Total R" value={rText(s().totalR)} tone={tone(s().totalR)} sub={`${rText(s().avgR)} a trade`} />
        <Card label="Profit factor" value={s().profitFactor === null ? '—' : s().profitFactor!.toFixed(2)} sub={`expectancy ${money(s().expectancy)}`} />
        <Card label="Avg win / loss" value={money(s().avgWin)} sub={money(-s().avgLoss)} />
        <Card label="Max drawdown" value={money(-s().maxDrawdown)} tone={s().maxDrawdown > 0 ? 'bear' : ''} sub={`best ${money(s().best)} · worst ${money(s().worst)}`} />
        <Card label="Streak" value={s().streak > 0 ? `${s().streak} ${s().streakKind === 'win' ? (s().streak > 1 ? 'wins' : 'win') : s().streak > 1 ? 'losses' : 'loss'}` : '—'} tone={s().streakKind === 'win' ? 'bull' : s().streakKind === 'loss' ? 'bear' : ''} sub={s().avgHoldHours === null ? undefined : `held ${hold(s().avgHoldHours!)} on average`} />
        <Card label="Trades" value={`${s().closed} closed`} sub={`${s().open} open · ${s().planned} planned · ${s().longs} long · ${s().shorts} short`} />
      </div>
      <div class="jr-grid">
        <section class="jr-box jr-wide">
          <h3>Equity</h3>
          <Equity trades={list()} />
        </section>
        <section class="jr-box">
          <h3>P&L calendar</h3>
          <Calendar trades={list()} />
        </section>
        <section class="jr-box">
          <h3>By pair</h3>
          <Groups groups={groupBy(list(), (t) => symbolOf(t.market))} />
        </section>
        <section class="jr-box">
          <h3>By side</h3>
          <Groups groups={groupBy(list(), (t) => (t.side === 'long' ? 'Long' : 'Short'))} />
        </section>
        <section class="jr-box">
          <h3>By strategy</h3>
          <Groups groups={groupBy(list(), (t) => t.strategy)} />
        </section>
      </div>
    </div>
  );
}

const hold = (hours: number): string => (hours < 1 ? `${Math.round(hours * 60)}m` : hours < 48 ? `${hours.toFixed(1)}h` : `${(hours / 24).toFixed(1)}d`);

function Card(props: { label: string; value: string; sub?: string; tone?: string }) {
  return (
    <div class="jr-card">
      <span class="cap">{props.label}</span>
      <span class={`jr-value num ${props.tone ?? ''}`}>{props.value}</span>
      <Show when={props.sub}>
        <span class="dim num">{props.sub}</span>
      </Show>
    </div>
  );
}

/** The running total of closed trades, as a line over zero. */
function Equity(props: { trades: Trade[] }) {
  const W = 600;
  const H = 150;
  const curve = createMemo(() => equityCurve(props.trades));
  const shape = createMemo(() => {
    const c = curve();
    if (c.length < 2) return null;
    const totals = [0, ...c.map((p) => p.total)];
    const lo = Math.min(...totals);
    const hi = Math.max(...totals);
    const span = hi - lo || 1;
    const y = (v: number) => H - 6 - ((v - lo) / span) * (H - 12);
    const x = (i: number) => (i / c.length) * W;
    const points = [`0,${y(0)}`, ...c.map((p, i) => `${x(i + 1)},${y(p.total)}`)].join(' ');
    return { points, zero: y(0), last: c[c.length - 1]!.total };
  });
  return (
    <Show when={shape()} fallback={<p class="dim">The curve draws itself once two trades have closed.</p>}>
      {(s) => (
        <svg class="jr-equity" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="img" aria-label={`Equity: ${money(s().last)}`}>
          <line x1="0" x2={W} y1={s().zero} y2={s().zero} class="jr-zero" />
          <polyline points={s().points} class={s().last >= 0 ? 'jr-line bull' : 'jr-line bear'} vector-effect="non-scaling-stroke" />
        </svg>
      )}
    </Show>
  );
}

const WEEKDAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];
const MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];

/** A month of days, each with what its closed trades made. */
function Calendar(props: { trades: Trade[] }) {
  const today = new Date();
  const [month, setMonth] = createSignal({ y: today.getFullYear(), m: today.getMonth() });
  const days = createMemo(() => dailyPnl(props.trades));
  const cells = createMemo(() => {
    const { y, m } = month();
    const first = new Date(y, m, 1);
    const lead = (first.getDay() + 6) % 7;
    const count = new Date(y, m + 1, 0).getDate();
    return [...Array<null>(lead).fill(null), ...Array.from({ length: count }, (_, i) => localDay(new Date(y, m, i + 1).getTime()))];
  });
  const total = createMemo(() => cells().reduce((sum, day) => sum + (day ? (days().get(day)?.pnl ?? 0) : 0), 0));
  const biggest = createMemo(() => Math.max(1, ...cells().map((day) => Math.abs(day ? (days().get(day)?.pnl ?? 0) : 0))));
  const step = (n: number) => setMonth(({ y, m }) => ({ y: m + n < 0 ? y - 1 : m + n > 11 ? y + 1 : y, m: (m + n + 12) % 12 }));
  return (
    <div class="jr-cal">
      <div class="jr-cal-head">
        <button class="iconbtn" aria-label="Previous month" onClick={() => step(-1)}>
          ‹
        </button>
        <span class="grow">
          {MONTHS[month().m]} {month().y}
        </span>
        <span class={`num ${tone(total())}`}>{money(total())}</span>
        <button class="iconbtn" aria-label="Next month" onClick={() => step(1)}>
          ›
        </button>
      </div>
      <div class="jr-cal-grid">
        <For each={WEEKDAYS}>{(d) => <span class="dim jr-wd">{d}</span>}</For>
        <For each={cells()}>
          {(day) => {
            const d = () => (day ? days().get(day) : undefined);
            const strength = () => Math.min(1, Math.abs(d()?.pnl ?? 0) / biggest());
            return (
              <div
                class="jr-day"
                classList={{ blank: !day, up: (d()?.pnl ?? 0) > 0, down: (d()?.pnl ?? 0) < 0 }}
                style={{ '--s': String(0.12 + strength() * 0.5) }}
                data-tip={d() ? `${day}: ${money(d()!.pnl)} from ${d()!.trades} trade${d()!.trades > 1 ? 's' : ''}` : undefined}
              >
                <Show when={day}>
                  <span class="dim">{Number(day!.slice(8))}</span>
                  <Show when={d()}>
                    <span class="num">{compactMoney(d()!.pnl)}</span>
                  </Show>
                </Show>
              </div>
            );
          }}
        </For>
      </div>
    </div>
  );
}

/** `+1.2K`, `−85`: a calendar day's figure, short. */
function compactMoney(v: number): string {
  const sign = v > 0 ? '+' : v < 0 ? '−' : '';
  const a = Math.abs(v);
  return `${sign}${a >= 1000 ? `${(a / 1000).toFixed(1)}K` : a.toFixed(a >= 100 ? 0 : 1)}`;
}

function Groups(props: { groups: Group[] }) {
  return (
    <Show when={props.groups.length > 0} fallback={<p class="dim">Nothing closed yet.</p>}>
      <table class="jr-table jr-groups">
        <thead>
          <tr>
            <th />
            <th>Trades</th>
            <th>Win</th>
            <th>R</th>
            <th>P&L</th>
          </tr>
        </thead>
        <tbody>
          <For each={props.groups.slice(0, 12)}>
            {(g) => (
              <tr>
                <td>{g.key}</td>
                <td class="num">{g.trades}</td>
                <td class="num">{g.winRate.toFixed(0)}%</td>
                <td class={`num ${tone(g.totalR)}`}>{rText(g.totalR)}</td>
                <td class={`num ${tone(g.pnl)}`}>{money(g.pnl)}</td>
              </tr>
            )}
          </For>
        </tbody>
      </table>
    </Show>
  );
}

// ---------------------------------------------------------------- history

type Filter = 'all' | 'live' | 'closed' | 'win' | 'loss';
const FILTERS: [Filter, string][] = [
  ['all', 'All'],
  ['live', 'Open & planned'],
  ['closed', 'Closed'],
  ['win', 'Won'],
  ['loss', 'Lost'],
];

function History(props: { trades: Trade[]; price: (market: string, v: number | null) => string; onSelect: (id: number) => void }) {
  const [filter, setFilter] = createSignal<Filter>('all');
  const [side, setSide] = createSignal<'all' | Side>('all');
  const [query, setQuery] = createSignal('');
  const rows = createMemo(() => {
    const q = query().trim().toUpperCase();
    return props.trades
      .filter((t) => {
        const f = filter();
        if (f === 'live' && !isLive(t)) return false;
        if (f === 'closed' && !isClosed(t)) return false;
        if (f === 'win' && !((pnl(t) ?? 0) > 0)) return false;
        if (f === 'loss' && !((pnl(t) ?? 0) < 0)) return false;
        if (side() !== 'all' && t.side !== side()) return false;
        return !q || t.market.toUpperCase().includes(q) || t.strategy.toUpperCase().includes(q) || t.notes.toUpperCase().includes(q);
      })
      .sort((a, b) => when(b) - when(a));
  });
  return (
    <div class="jr-hist">
      <div class="jr-filters">
        <div class="chips">
          <For each={FILTERS}>
            {([id, label]) => (
              <button classList={{ on: filter() === id }} onClick={() => setFilter(id)}>
                {label}
              </button>
            )}
          </For>
        </div>
        <div class="chips">
          <For each={['all', 'long', 'short'] as const}>
            {(id) => (
              <button classList={{ on: side() === id }} onClick={() => setSide(id)}>
                {id === 'all' ? 'Both' : id === 'long' ? 'Long' : 'Short'}
              </button>
            )}
          </For>
        </div>
        <input class="jr-search" placeholder="Search market, strategy, notes" value={query()} onInput={(e) => setQuery(e.currentTarget.value)} />
        <span class="dim">{rows().length} trades</span>
      </div>
      <div class="jr-scroll">
        <table class="jr-table jr-trades">
          <thead>
            <tr>
              <th>Date</th>
              <th>Market</th>
              <th>Side</th>
              <th>Entry</th>
              <th>Stop</th>
              <th>Target</th>
              <th>RR</th>
              <th>Exit</th>
              <th>Status</th>
              <th>R</th>
              <th>P&L</th>
              <th>Strategy</th>
            </tr>
          </thead>
          <tbody>
            <For each={rows()}>
              {(t) => (
                <tr onClick={() => props.onSelect(t.id)}>
                  <td class="num dim">{stamp(when(t))}</td>
                  <td>
                    {symbolOf(t.market)} <span class="dim">{venueLabel(t.market)}</span>
                  </td>
                  <td class={t.side === 'long' ? 'bull' : 'bear'}>{t.side === 'long' ? 'Long' : 'Short'}</td>
                  <td class="num">{props.price(t.market, t.entry)}</td>
                  <td class="num">{props.price(t.market, t.stop)}</td>
                  <td class="num">{props.price(t.market, t.target)}</td>
                  <td class="num">{plannedRR(t).toFixed(2)}</td>
                  <td class="num">{props.price(t.market, t.exit)}</td>
                  <td>
                    <StatusBadge trade={t} />
                  </td>
                  <td class={`num ${tone(rMultiple(t))}`}>{rText(rMultiple(t))}</td>
                  <td class={`num ${tone(pnl(t))}`}>{money(pnl(t))}</td>
                  <td class="dim">{t.strategy}</td>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </div>
    </div>
  );
}

function StatusBadge(props: { trade: Trade }) {
  const kind = () => {
    const t = props.trade;
    if (t.status === 'win' || (t.status === 'closed' && (pnl(t) ?? 0) > 0)) return 'bull';
    if (t.status === 'loss' || (t.status === 'closed' && (pnl(t) ?? 0) < 0)) return 'bear';
    return '';
  };
  return (
    <span class={`badge ${kind()}`} data-tip={props.trade.auto ? (props.trade.tracked ? 'Price decides this one' : 'Kept by hand: this market’s prices are not read here') : 'Set by hand'}>
      {STATUS_LABEL[props.trade.status]}
    </span>
  );
}

// ---------------------------------------------------------------- one trade

function Editor(props: {
  trade: Trade;
  price: (market: string, v: number | null) => string;
  onClose: () => void;
  onChange: (t: Trade) => void;
  onRemove: (id: number) => void;
}) {
  const t = () => props.trade;
  const [strategy, setStrategy] = createSignal(t().strategy);
  const [notes, setNotes] = createSignal(t().notes);
  const [fees, setFees] = createSignal(String(t().fees || ''));
  const [risk, setRisk] = createSignal(String(Number(t().risk.toFixed(2))));
  const [exit, setExit] = createSignal('');
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [sure, setSure] = createSignal(false);

  const act = async (run: () => Promise<Trade | void>) => {
    setBusy(true);
    setError(null);
    try {
      const done = await run();
      if (done) props.onChange(done);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  const dirty = () => strategy() !== t().strategy || notes() !== t().notes || Number(fees() || 0) !== t().fees || Number(risk()) !== Number(t().risk.toFixed(2));
  const save = () =>
    act(() => journalApi.update(t().id, { strategy: strategy(), notes: notes(), fees: Number(fees() || 0), ...(Number(risk()) > 0 && Number(risk()) !== Number(t().risk.toFixed(2)) ? { risk: Number(risk()) } : {}) }));
  const openChart = () => {
    const chart = targetChart();
    if (!chart) return;
    if (marketOf(chart) !== t().market) setPanelMarket(chart.id, t().market);
    if (t().timeframe) setPanelTimeframe(chart.id, t().timeframe);
    focusPanel(chart.id);
    setJournalOpen(false);
  };
  const coin = () => symbolOf(t().market).replace(/(USDT|USDC|USD)$/, '') || 'units';
  const qtyOf = (q: number) => `${Number(q.toPrecision(4))} ${coin()}`;
  const figures = (): [string, string, string?][] => [
    ['Entry', props.price(t().market, t().entry)],
    ['Stop', props.price(t().market, t().stop)],
    ['Target', props.price(t().market, t().target)],
    ['Planned RR', plannedRR(t()).toFixed(2)],
    ['Risk', money(t().risk).replace('+', '')],
    ['Size', qtyOf(t().qty)],
    ['Drawn', stamp(t().startAt)],
    ['Opened', stamp(t().openedAt)],
    ['Closed', stamp(t().closedAt)],
    ...(t().fills.length > 1 ? ([['Avg entry', props.price(t().market, book(t()).avgEntry)], ['Held', qtyOf(book(t()).held)]] as [string, string][]) : []),
    [t().fills.filter((f) => !IN.has(f.kind)).length > 1 ? 'Avg exit' : 'Exit', props.price(t().market, t().exit)],
    ['R', rText(rMultiple(t())), tone(rMultiple(t()))],
    ['P&L', money(pnl(t())), tone(pnl(t()))],
  ];
  return (
    <>
      <div class="jr-scrim" onClick={props.onClose} />
      <aside class="jr-drawer">
        <header class="jr-head">
          <span class={t().side === 'long' ? 'bull' : 'bear'}>{t().side === 'long' ? 'Long' : 'Short'}</span>
          <span class="jr-title">{symbolOf(t().market)}</span>
          <span class="dim">{venueLabel(t().market)}</span>
          <StatusBadge trade={t()} />
          <span class="grow" />
          <button class="iconbtn" aria-label="Close" onClick={props.onClose}>
            <Icon name="close" size={15} />
          </button>
        </header>
        <div class="jr-drawer-body">
          <dl class="jr-figures">
            <For each={figures()}>
              {([label, value, cls]) => (
                <>
                  <dt class="dim">{label}</dt>
                  <dd class={`num ${cls ?? ''}`}>{value}</dd>
                </>
              )}
            </For>
          </dl>
          <Show when={t().fills.length > 1 || t().legs.length > 0}>
            <ol class="jr-fills num">
              <For each={t().fills}>
                {(f) => {
                  const b = book(t());
                  const made = IN.has(f.kind) ? null : (f.price - b.avgEntry) * (t().side === 'long' ? 1 : -1) * f.qty;
                  return (
                    <li>
                      <span class="dim">{stamp(f.at)}</span>
                      <span>{FILL_LABEL[f.kind]}</span>
                      <span>{IN.has(f.kind) ? '+' : '−'}{qtyOf(f.qty)}</span>
                      <span>@ {props.price(t().market, f.price)}</span>
                      <span class={tone(made)}>{made === null ? '' : money(made)}</span>
                    </li>
                  );
                }}
              </For>
              <For each={t().legs.filter((_, i) => !t().fills.some((f) => f.leg === i))}>
                {(l) => (
                  <li class="dim">
                    <span>{isLive(t()) ? 'waiting' : 'not reached'}</span>
                    <span>{l.kind === 'take' ? 'Take profit' : l.kind === 'cut' ? 'Cut loss' : 'Add'}</span>
                    <span>{Math.round(l.share * 100)}%</span>
                    <span>@ {props.price(t().market, l.price)}</span>
                    <span />
                  </li>
                )}
              </For>
            </ol>
          </Show>
          <p class="dim jr-note">
            {t().drawingId !== null ? `Follows its ${t().timeframe} position on the chart: moving it moves the trade.` : t().timeframe ? 'Its position was deleted from the chart; the trade stays.' : 'Added by hand.'}{' '}
            {t().auto
              ? t().tracked
                ? `Price decides it: open at the entry${t().legs.length > 0 ? ', its legs filled as price reaches them' : ''}, closed at the target or the stop.`
                : 'This market’s prices are not read here: set how it went by hand.'
              : 'Its status was set by hand.'}
          </p>

          <Show when={isLive(t()) || !t().auto}>
            <div class="jr-actions">
              <Show when={isLive(t())}>
                <input class="jr-in num" inputmode="decimal" placeholder="Exit price" value={exit()} onInput={(e) => setExit(e.currentTarget.value)} />
                <button class="tb" disabled={busy() || !(Number(exit()) > 0)} onClick={() => act(() => journalApi.update(t().id, { status: 'closed', exit: Number(exit()) }))}>
                  Close at it
                </button>
                <button class="tb" disabled={busy()} onClick={() => act(() => journalApi.update(t().id, { status: 'breakeven' }))}>
                  Break-even
                </button>
                <button class="tb" disabled={busy()} onClick={() => act(() => journalApi.update(t().id, { status: 'cancelled' }))}>
                  Cancel
                </button>
              </Show>
              <Show when={!t().auto && t().tracked}>
                <button class="tb" disabled={busy()} data-tip="Weigh it against price again, from where it was drawn" onClick={() => act(() => journalApi.update(t().id, { auto: true }))}>
                  Let price decide
                </button>
              </Show>
            </div>
          </Show>

          <StrategyPicker value={strategy()} onChange={setStrategy} />
          <div class="jr-row">
            <label class="jr-label">
              Risk ($)
              <input class="jr-in num" inputmode="decimal" value={risk()} onInput={(e) => setRisk(e.currentTarget.value)} />
            </label>
            <label class="jr-label">
              Fees ($)
              <input class="jr-in num" inputmode="decimal" value={fees()} onInput={(e) => setFees(e.currentTarget.value)} />
            </label>
          </div>
          <label class="jr-label">
            Notes
            <textarea class="jr-in" rows={6} value={notes()} onInput={(e) => setNotes(e.currentTarget.value)} />
          </label>
          <Show when={error()}>{(e) => <p class="bear">{e()}</p>}</Show>
          <div class="jr-actions">
            <button class="tb on" disabled={busy() || !dirty()} onClick={save}>
              Save
            </button>
            <button class="tb" onClick={openChart}>
              <Icon name="external" size={12} />
              <span>Open chart</span>
            </button>
            <span class="grow" />
            <button
              class="tb jr-delete"
              classList={{ sure: sure() }}
              disabled={busy()}
              onClick={() => (sure() ? act(async () => void (await journalApi.remove(t().id), props.onRemove(t().id))) : setSure(true))}
            >
              <Icon name="trash" size={12} />
              <span>{sure() ? 'Delete from the journal' : 'Delete'}</span>
            </button>
          </div>
        </div>
      </aside>
    </>
  );
}

// ---------------------------------------------------------------- by hand

function AddTrade(props: { onClose: () => void; onAdd: (t: Trade) => void }) {
  const chart = targetChart();
  const [market, setMarket] = createSignal(chart ? marketOf(chart) : 'binance-perp:BTCUSDT');
  const [picking, setPicking] = createSignal(false);
  const [side, setSide] = createSignal<Side>('long');
  const [entry, setEntry] = createSignal('');
  const [stop, setStop] = createSignal('');
  const [target, setTarget] = createSignal('');
  const [risk, setRisk] = createSignal('10');
  const [at, setAt] = createSignal(localInput(Date.now()));
  const [closed, setClosed] = createSignal(false);
  const [exit, setExit] = createSignal('');
  const [closedAt, setClosedAt] = createSignal(localInput(Date.now()));
  const [strategy, setStrategy] = createSignal('');
  const [notes, setNotes] = createSignal('');
  const [error, setError] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);

  const submit = async (e: Event) => {
    e.preventDefault();
    if (!isMarket(market())) {
      setError('Market is venue:SYMBOL, as a chart names it — binance-perp:BTCUSDT');
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const status: Status = closed() ? 'closed' : 'planned';
      props.onAdd(
        await journalApi.create({
          market: market(),
          side: side(),
          entry: Number(entry()),
          stop: Number(stop()),
          target: Number(target()),
          risk: Number(risk()) || undefined,
          startAt: Date.parse(at()),
          status,
          ...(closed() ? { exit: Number(exit()), closedAt: Date.parse(closedAt()) } : {}),
          strategy: strategy(),
          notes: notes(),
        }),
      );
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div class="backdrop" onClick={(e) => e.target === e.currentTarget && props.onClose()}>
      <form class="dialog jr-form" onSubmit={submit}>
        <header class="jr-head">
          <span class="jr-title">Add a trade</span>
          <span class="grow" />
          <button type="button" class="iconbtn" aria-label="Close" onClick={props.onClose}>
            <Icon name="close" size={15} />
          </button>
        </header>
        <div class="jr-drawer-body">
          <div class="jr-label">
            Market
            <button type="button" class="jr-in jr-market" data-tip="Search every market, as the chart's symbol search does" onClick={() => setPicking(true)}>
              <span class="sym">{symbolOf(market())}</span>
              <VenueTag market={market()} name />
              <span class="grow" />
              <Icon name="search" size={12} />
            </button>
          </div>
          <Show when={picking()}>
            {/* In a portal: closing the search on a press outside must not land that click on this dialog's backdrop. */}
            <Portal>
              <SymbolSearch
                initial=""
                title="Market"
                current={market()}
                onClose={() => setPicking(false)}
                onSelect={(m) => {
                  setMarket(m);
                  setPicking(false);
                }}
              />
            </Portal>
          </Show>
          <div class="chips">
            <button type="button" classList={{ on: side() === 'long' }} onClick={() => setSide('long')}>
              Long
            </button>
            <button type="button" classList={{ on: side() === 'short' }} onClick={() => setSide('short')}>
              Short
            </button>
          </div>
          <div class="jr-row">
            <label class="jr-label">
              Entry
              <input class="jr-in num" inputmode="decimal" required value={entry()} onInput={(e) => setEntry(e.currentTarget.value)} />
            </label>
            <label class="jr-label">
              Stop
              <input class="jr-in num" inputmode="decimal" required value={stop()} onInput={(e) => setStop(e.currentTarget.value)} />
            </label>
            <label class="jr-label">
              Target
              <input class="jr-in num" inputmode="decimal" required value={target()} onInput={(e) => setTarget(e.currentTarget.value)} />
            </label>
          </div>
          <div class="jr-row">
            <label class="jr-label">
              Risk ($)
              <input class="jr-in num" inputmode="decimal" value={risk()} onInput={(e) => setRisk(e.currentTarget.value)} />
            </label>
            <label class="jr-label">
              Entered
              <input class="jr-in" type="datetime-local" value={at()} onInput={(e) => setAt(e.currentTarget.value)} />
            </label>
          </div>
          <label class="jr-check">
            <input type="checkbox" checked={closed()} onChange={(e) => setClosed(e.currentTarget.checked)} /> Already closed
          </label>
          <Show when={closed()}>
            <div class="jr-row">
              <label class="jr-label">
                Exit
                <input class="jr-in num" inputmode="decimal" required value={exit()} onInput={(e) => setExit(e.currentTarget.value)} />
              </label>
              <label class="jr-label">
                Closed
                <input class="jr-in" type="datetime-local" value={closedAt()} onInput={(e) => setClosedAt(e.currentTarget.value)} />
              </label>
            </div>
          </Show>
          <StrategyPicker value={strategy()} onChange={setStrategy} />
          <label class="jr-label">
            Notes
            <textarea class="jr-in" rows={3} value={notes()} onInput={(e) => setNotes(e.currentTarget.value)} />
          </label>
          <p class="dim jr-note">{closed() ? 'Kept as you set it.' : 'Price decides it from when it was entered, where this market’s prices are read.'}</p>
          <Show when={error()}>{(e) => <p class="bear">{e()}</p>}</Show>
          <div class="jr-actions">
            <button class="tb on" type="submit" disabled={busy()}>
              Add to the journal
            </button>
          </div>
        </div>
      </form>
    </div>
  );
}

// ---------------------------------------------------------------- review

function ReviewTab(props: { trades: Trade[] }) {
  const weeks = lastWeeks(12).reverse();
  const [week, setWeek] = createSignal(weekOf(Date.now()).key);
  const [reviews, setReviews] = createSignal(new Map<string, Review>());
  const [draft, setDraft] = createSignal<Review['answers']>({});
  const [state, setState] = createSignal<'idle' | 'saving' | 'saved'>('idle');
  const [error, setError] = createSignal<string | null>(null);
  onMount(() => {
    void journalApi
      .reviews()
      .then((list) => {
        setReviews(new Map(list.map((r) => [r.week, r])));
        setDraft({ ...(reviews().get(week())?.answers ?? {}) });
      })
      .catch((e) => setError(errorText(e)));
  });
  const pick = (key: string) => {
    setWeek(key);
    setDraft({ ...(reviews().get(key)?.answers ?? {}) });
    setState('idle');
  };
  const current = () => weeks.find((w) => w.key === week()) ?? weeks[0]!;
  const s = createMemo(() => stats(inWeek(props.trades, current())));
  const save = async () => {
    setState('saving');
    setError(null);
    try {
      const saved = await journalApi.saveReview(week(), draft());
      setReviews((m) => new Map(m).set(week(), { week: week(), answers: draft(), savedAt: saved.savedAt }));
      setState('saved');
    } catch (e) {
      setError(errorText(e));
      setState('idle');
    }
  };
  return (
    <div class="jr-review">
      <nav class="jr-weeks">
        <For each={weeks}>
          {(w) => (
            <button classList={{ cur: w.key === week() }} onClick={() => pick(w.key)}>
              <span class="grow">{w.key.replace('-W', ' · W')}</span>
              <span class="dim">{w.start.slice(5)}</span>
              <Show when={reviews().has(w.key)}>
                <span class="bull" aria-label="reviewed">
                  ✓
                </span>
              </Show>
            </button>
          )}
        </For>
      </nav>
      <div class="jr-review-body">
        <div class="jr-cards">
          <Card label={`${current().start} – ${current().end}`} value={money(s().totalPnl)} tone={tone(s().totalPnl)} sub={`${s().closed} closed · ${s().total} in all`} />
          <Card label="Win rate" value={`${s().winRate.toFixed(0)}%`} sub={`${s().wins} won · ${s().losses} lost`} />
          <Card label="Total R" value={rText(s().totalR)} tone={tone(s().totalR)} />
        </div>
        <For each={QUESTIONS}>
          {(q) => (
            <label class="jr-q">
              <span>
                <span class="badge">{q.badge}</span> {q.question}
              </span>
              <span class="dim">{q.prompt}</span>
              <textarea
                class="jr-in"
                rows={3}
                value={draft()[q.id as AnswerKey] ?? ''}
                onInput={(e) => {
                  const v = e.currentTarget.value;
                  setDraft((d) => ({ ...d, [q.id]: v }));
                  setState('idle');
                }}
              />
            </label>
          )}
        </For>
        <Show when={error()}>{(e) => <p class="bear">{e()}</p>}</Show>
        <div class="jr-actions">
          <button class="tb on" disabled={state() === 'saving'} onClick={() => void save()}>
            {state() === 'saving' ? 'Saving…' : 'Save the review'}
          </button>
          <Show when={state() === 'saved'}>
            <span class="dim">Saved.</span>
          </Show>
        </div>
      </div>
    </div>
  );
}
