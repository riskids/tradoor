/**
 * A long or short position's Inputs, laid out as rsalianto's trade form: the
 * strategy it was taken on, its prices and size, the legs drawn with it —
 * partial take profits, partial cut losses, added entries — and, for a
 * browser signed in to keep a journal, its notes and a close by hand. The
 * prices, the size and the legs are the drawing's and show on the chart as
 * they change; the strategy, the notes and the close are the journal's
 * trade's, saved once the drawing is.
 */
import { createMemo, createSignal, For, type JSX, onMount, Show } from 'solid-js';
import type { DrawingPoint } from '../engine/protocol';
import { api } from '../lib/api';
import type { Style } from '../lib/drawing-style';
import { Icon } from '../lib/icons';
import { fits, fullEntry, MAX_LEGS, parseLegs, runTo, serializeLegs, type DrawnLeg } from '../journal/legs';
import { book, FILL_LABEL, journalApi, localInput, money, pnl, rMultiple, rText, stamp, STATUS_LABEL, type LegKind, type Trade, type TradePatch } from '../journal/model';
import { StrategyPicker } from '../journal/strategies';

const LEG_GROUPS: { kind: LegKind; title: string; add: string; tip: string }[] = [
  { kind: 'take', title: 'Partial take profit', add: 'Add TP', tip: 'Part of the position taken off on the way to the target' },
  { kind: 'cut', title: 'Partial cut loss', add: 'Add cut', tip: 'Part of the position cut on the way to the stop' },
  { kind: 'add', title: 'Average entry', add: 'Add entry', tip: 'More bought (sold, for a short) at another price, averaging the entry' },
];
const WHERE: Record<LegKind, string> = {
  take: 'between the entry and the target',
  cut: 'between the entry and the stop',
  add: 'between the stop and the target',
};

export function PositionTicket(props: {
  drawingId: number;
  kind: string;
  style: Style;
  points: DrawingPoint[];
  precision: number;
  set(key: string, v: string | number): void;
  merge(values: Style): void;
  setPrice(i: number, price: number): void;
  num(key: string, fallback: number): number;
  str(key: string, fallback?: string): string;
  /** Ok saves the drawing, then calls what this hands it. */
  register(save: () => Promise<void>): void;
}) {
  const tick = () => 10 ** -props.precision;
  const long = props.kind === 'long';
  const dir = long ? 1 : -1;
  const entry = () => props.points[0]?.price ?? 0;
  const stop = () => props.points[1]?.price ?? 0;
  const target = () => props.points[2]?.price ?? 0;
  const round = (v: number) => Number(v.toFixed(props.precision));
  const ticks = (price: number) => Math.round(Math.abs(price - entry()) / tick());
  const movePct = (price: number) => (entry() > 0 ? ((price - entry()) / entry()) * 100 : 0);
  const lot = () => props.num('lot', 1);
  const riskAmount = () => (props.str('riskUnit', '%') === '%' ? (props.num('account', 1000) * props.num('risk', 1)) / 100 : props.num('risk', 10));
  const qty = () => {
    const per = Math.abs(entry() - stop()) * lot();
    return per > 0 ? riskAmount() / per : 0;
  };
  const coins = () => qty() * lot();
  const qtyText = (q: number) => {
    const d = props.str('qtyDecimals', '');
    return d === '' ? String(Number(q.toPrecision(6))) : q.toFixed(Number(d));
  };
  const rr = () => {
    const r = Math.abs(entry() - stop());
    return r > 0 ? Math.abs(target() - entry()) / r : 0;
  };
  const margin = () => (coins() * entry()) / props.num('leverage', 1);

  // The legs, as the drawing keeps them.
  const legs = createMemo(() => parseLegs(props.style.legs));
  const live = createMemo(() => legs().filter((l) => fits(l, entry(), stop(), target())));
  const setLegs = (list: DrawnLeg[]) => props.set('legs', serializeLegs(list));
  // A new leg starts on its own side, apart from the others: a take halfway to the target, an
  // add a third of the way to the stop, a cut two thirds of it.
  const addLeg = (kind: LegKind) => {
    const [end, part] = kind === 'take' ? [target(), 1 / 2] : kind === 'add' ? [stop(), 1 / 3] : [stop(), 2 / 3];
    setLegs([...legs(), { kind, price: round(entry() + (end - entry()) * part), percent: kind === 'add' ? 100 : 50 }]);
  };
  const editLeg = (i: number, change: Partial<DrawnLeg>) => setLegs(legs().map((l, j) => (j === i ? { ...l, ...change } : l)));
  const removeLeg = (i: number) => setLegs(legs().filter((_, j) => j !== i));
  const made = (end: number) => runTo(entry(), end, coins(), live(), dir);
  const hasAdds = () => live().some((l) => l.kind === 'add');

  // The journal's side: kept for a browser signed in with Telegram.
  const [signedIn, setSignedIn] = createSignal<boolean | null>(null);
  const [trade, setTrade] = createSignal<Trade | null>(null);
  const [strategy, setStrategy] = createSignal('');
  const [notes, setNotes] = createSignal('');
  const [exitPrice, setExitPrice] = createSignal('');
  const [exitAt, setExitAt] = createSignal(localInput(Date.now()));
  const mine = (list: Trade[]) => list.find((t) => t.drawingId === props.drawingId) ?? null;
  onMount(() => {
    void api
      .account()
      .then(async (a) => {
        setSignedIn(!!a.account);
        if (!a.account) return;
        const t = mine(await journalApi.trades());
        setTrade(t);
        setStrategy(t?.strategy ?? '');
        setNotes(t?.notes ?? '');
      })
      .catch(() => setSignedIn(false));
  });
  props.register(async () => {
    if (!signedIn()) return;
    // A position drawn before signing in has its trade once the drawing is saved.
    const t = trade() ?? mine(await journalApi.trades());
    if (!t) return;
    const patch: TradePatch = {};
    if (strategy() !== t.strategy) patch.strategy = strategy();
    if (notes() !== t.notes) patch.notes = notes();
    const x = Number(exitPrice());
    if (exitPrice().trim() !== '' && x > 0) Object.assign(patch, { status: 'closed', exit: x, closedAt: new Date(exitAt()).getTime() || Date.now() });
    if (Object.keys(patch).length > 0) await journalApi.update(t.id, patch);
  });
  const settled = () => {
    const t = trade();
    return t !== null && t.status !== 'planned' && t.status !== 'open';
  };

  const numberInput = (value: number, onChange: (v: number) => void, step: string | number = 'any', label?: string) => (
    <input
      class="dinput num"
      type="number"
      step={step}
      value={value}
      aria-label={label}
      onChange={(e) => {
        const v = Number(e.currentTarget.value);
        if (Number.isFinite(v)) onChange(v);
      }}
    />
  );
  const field = (label: string, input: JSX.Element, hint?: () => string) => (
    <label class="tfield">
      <span class="tlabel">{label}</span>
      <div class="trow">{input}</div>
      <Show when={hint?.()}>{(h) => <span class="thint num">{h()}</span>}</Show>
    </label>
  );
  const distance = (price: number) => `${ticks(price).toLocaleString('en-US')} ticks · ${movePct(price) > 0 ? '+' : ''}${movePct(price).toFixed(2)}%`;

  return (
    <div class="ticket">
      <Show
        when={signedIn() !== false}
        fallback={<p class="tnote">Strategy, notes and the journal are kept for a Telegram account: sign in from the Journal in the top bar, and every position you draw is written down.</p>}
      >
        <StrategyPicker value={strategy()} onChange={setStrategy} />
      </Show>

      <div class="ticket-cols">
        <div class="ticket-col">
          {field('Entry price', numberInput(round(entry()), (v) => props.setPrice(0, v), tick(), 'Entry price'))}
          {field('Stop loss', numberInput(round(stop()), (v) => props.setPrice(1, v), tick(), 'Stop loss'), () => distance(stop()))}
          {field('Take profit', numberInput(round(target()), (v) => props.setPrice(2, v), tick(), 'Take profit'), () => distance(target()))}
          {field(
            'Risk',
            <>
              {numberInput(props.num('risk', 1), (v) => v > 0 && props.set('risk', v), 'any', 'Risk')}
              <select class="dinput narrow" value={props.str('riskUnit', '%')} onChange={(e) => props.set('riskUnit', e.currentTarget.value)}>
                <option value="%">%</option>
                <option value="cash">Cash</option>
              </select>
            </>,
          )}
          {field(
            'Position size (qty)',
            numberInput(
              Number(qtyText(qty())),
              // A size typed in sets the risk it takes, in cash.
              (v) => v > 0 && props.merge({ riskUnit: 'cash', risk: Number((v * Math.abs(entry() - stop()) * lot()).toFixed(8)) }),
              'any',
              'Position size',
            ),
            () => `Margin ${margin().toFixed(2)} at ${props.num('leverage', 1)}×`,
          )}
          <div class="tpair">
            {field('Account size', numberInput(props.num('account', 1000), (v) => v > 0 && props.set('account', v)))}
            {field('Leverage', numberInput(props.num('leverage', 1), (v) => v > 0 && props.set('leverage', v)))}
          </div>
          <div class="tpair">
            {field('Lot size', numberInput(lot(), (v) => v > 0 && props.set('lot', v)))}
            {field(
              'Qty precision',
              <select class="dinput" value={props.str('qtyDecimals', '')} onChange={(e) => props.set('qtyDecimals', e.currentTarget.value)}>
                <option value="">Default</option>
                <For each={[0, 1, 2, 3, 4, 5, 6, 7, 8]}>{(d) => <option value={d}>{d}</option>}</For>
              </select>,
            )}
          </div>
        </div>

        <div class="ticket-rule" />

        <div class="ticket-col">
          <For each={LEG_GROUPS}>
            {(g) => (
              <div class="tlegs">
                <div class="tlegs-head" data-tip={g.tip}>
                  <span class="tlabel">{g.title}</span>
                  <button class="tadd" disabled={legs().length >= MAX_LEGS} onClick={() => addLeg(g.kind)}>
                    <Icon name="plus" size={11} /> {g.add}
                  </button>
                </div>
                <For each={legs().map((l, i) => ({ l, i })).filter((x) => x.l.kind === g.kind)}>
                  {(x, n) => {
                    const ok = () => fits(x.l, entry(), stop(), target());
                    const moved = () => (x.l.percent / 100) * coins();
                    return (
                      <div classList={{ tleg: true, bad: !ok() }} data-tip={ok() ? undefined : `Left out: a level here goes ${WHERE[g.kind]}`}>
                        <span class="tleg-name">{g.kind === 'take' ? 'TP' : g.kind === 'cut' ? 'Cut' : 'Add'}{n() + 1}</span>
                        {numberInput(round(x.l.price), (v) => editLeg(x.i, { price: v }), tick(), `${g.title} ${n() + 1} price`)}
                        <span class="tpct">
                          {numberInput(x.l.percent, (v) => v > 0 && v <= 1000 && editLeg(x.i, { percent: v }), 'any', `${g.title} ${n() + 1} percent`)}%
                        </span>
                        <button class="iconbtn" aria-label="Remove" onClick={() => removeLeg(x.i)}>
                          <Icon name="close" size={12} />
                        </button>
                        <span class="thint num" data-tip={g.kind === 'add' ? 'Coins added' : 'Coins moved, and what they make against the entry'}>
                          {g.kind === 'add' ? '+' : '−'}
                          {qtyText(moved() / lot())}
                          <Show when={g.kind !== 'add'}> · {money((x.l.price - entry()) * dir * moved())}</Show>
                        </span>
                      </div>
                    );
                  }}
                </For>
              </div>
            )}
          </For>

          <Show when={signedIn()}>
            <Show
              when={!settled()}
              fallback={
                <div class="tfield">
                  <span class="tlabel">Result</span>
                  <span class="num">
                    {STATUS_LABEL[trade()!.status]} {stamp(trade()!.closedAt)} · {money(pnl(trade()!))} · {rText(rMultiple(trade()!))}
                  </span>
                </div>
              }
            >
              <div class="tpair">
                {field('Exit price (optional)', <input class="dinput num" type="number" step={tick()} placeholder="Close by hand" value={exitPrice()} onInput={(e) => setExitPrice(e.currentTarget.value)} />)}
                {field('Exit date', <input class="dinput" type="datetime-local" value={exitAt()} onInput={(e) => setExitAt(e.currentTarget.value)} />)}
              </div>
            </Show>
            <label class="tfield">
              <span class="tlabel">Notes</span>
              <textarea class="dinput tnotes" rows={3} maxLength={4000} placeholder="Setup, emotions, lessons learned…" value={notes()} onInput={(e) => setNotes(e.currentTarget.value)} />
            </label>
          </Show>
        </div>
      </div>

      <div class="dsummary num">
        <span data-tip={live().some((l) => l.kind !== 'take') ? 'At the stop, every add and cut on the way filled' : undefined}>
          {live().some((l) => l.kind !== 'take') ? 'Max loss' : 'Risk'} {(-made(stop())).toFixed(2)}
        </span>
        <span>Qty {qtyText(qty())}</span>
        <span>R:R {rr().toFixed(2)}</span>
        <span>Margin {margin().toFixed(2)}</span>
        <span class="bull">Target {money(made(target()))}</span>
        <Show when={hasAdds()}>
          <span data-tip="With every add filled">Avg entry {round(fullEntry(entry(), coins(), live()))}</span>
        </Show>
      </div>
      <Show when={trade()}>
        {(t) => (
          <div class="tstatus num">
            <span class="tlabel">Journal</span>
            <span>{STATUS_LABEL[t().status]}</span>
            <Show when={t().status === 'planned'}>
              <span class="dim">waiting for the entry</span>
            </Show>
            <Show when={t().openedAt !== null}>
              <span class="dim">opened {stamp(t().openedAt)}</span>
            </Show>
            <For each={t().fills.filter((f) => f.kind !== 'entry')}>
              {(f) => (
                <span class="dim">
                  {FILL_LABEL[f.kind]} {qtyText(f.qty / lot())} @ {round(f.price)} · {stamp(f.at)}
                </span>
              )}
            </For>
            <Show when={t().status === 'open' && t().fills.length > 0}>
              <span class="dim">held {qtyText(book(t()).held / lot())}</span>
            </Show>
          </div>
        )}
      </Show>
    </div>
  );
}
