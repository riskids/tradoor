import { createSignal, For, onCleanup, Show } from 'solid-js';
import { api, errorText, type TelegramStatus } from '../lib/api';
import { OWNER_KEY, OWNER_PATTERN, ownerId } from '../lib/owner';
import { ALL_EXCHANGES, ExchangeLogo } from '../lib/exchanges';
import { writePref } from '../lib/prefs';
import { deviceZone, LOCAL_ZONE, offsetLabel, offsetMinutes, TIME_ZONE_KEY, timeZone } from '../lib/timezone';
import { CHART_THEME, FONT_PX, UI_THEME } from '../lib/theme';
import { formatDateTime } from '../lib/timezone';
import { doc, resetWorkspace, setThemeValue, update } from '../workspace/store';

function Swatch(props: { name: string; label: string; fallback: string }) {
  const value = () => doc.theme[props.name] ?? props.fallback;
  return (
    <label class="swatch">
      <input type="color" value={value()} onInput={(e) => setThemeValue(props.name, e.currentTarget.value)} />
      <span class="name">{props.label}</span>
      <span class="num dim">{value()}</span>
      <Show when={doc.theme[props.name]}>
        <button
          class="dim"
          data-tip="Back to the default"
          onClick={(e) => {
            e.preventDefault();
            setThemeValue(props.name, null);
          }}
        >
          ↺
        </button>
      </Show>
    </label>
  );
}

/** Local first — the default — then UTC, then every named zone with its offset now. */
function zoneChoices(): { value: string; label: string }[] {
  const now = Date.now();
  const named = (typeof Intl.supportedValuesOf === 'function' ? Intl.supportedValuesOf('timeZone') : [])
    .filter((z) => z !== 'UTC')
    .map((z) => ({ zone: z, offset: offsetMinutes(z, now) }))
    .sort((a, b) => a.offset - b.offset || a.zone.localeCompare(b.zone));
  return [
    { value: LOCAL_ZONE, label: `Local — ${deviceZone()} (${offsetLabel(offsetMinutes(LOCAL_ZONE, now))})` },
    { value: 'UTC', label: 'UTC' },
    ...named.map((z) => ({ value: z.zone, label: `${z.zone.replaceAll('_', ' ')} (${offsetLabel(z.offset)})` })),
  ];
}

/** How long a message stays in the chat. Telegram refuses to take back one older than 48 hours. */
const CLEARING = [
  { value: '47', label: '2 days, then cleared' },
  { value: '24', label: '1 day, then cleared' },
  { value: '6', label: '6 hours, then cleared' },
  { value: 'keep', label: 'Keep every message' },
];

/**
 * Alerts on Telegram. A bot cannot open a conversation, so the reader does:
 * the link carries a one-use code that tells the server which browser the chat
 * belongs to. While it is open the status is read again, so the chat appears
 * here the moment the bot has it.
 */
function TelegramRow() {
  const [status, setStatus] = createSignal<TelegramStatus | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const [note, setNote] = createSignal<string | null>(null);
  const [waiting, setWaiting] = createSignal(false);

  /** The clock the reader reads the page in, which their messages follow. */
  const clock = () => {
    const zone = timeZone() === LOCAL_ZONE ? deviceZone() : timeZone();
    return { zone, offsetMinutes: offsetMinutes(timeZone(), Date.now()) };
  };

  const load = async () => {
    try {
      const next = await api.telegram();
      // A code is good for fifteen minutes and spent once, so keep a fresh one
      // in hand: one when this opens, another after a chat takes the last.
      const linked = status()?.chats.length ?? 0;
      if (next.enabled && next.bot && (!invite() || next.chats.length !== linked)) void mint();
      setStatus(next);
      setError(null);
      // Their zone changed, or summer time moved it: the chats follow.
      const now = clock();
      if (next.chats.some((c) => c.offsetMinutes !== now.offsetMinutes || c.zone !== now.zone)) {
        await api.telegramClock(now);
        setStatus(await api.telegram());
      }
    } catch (e) {
      setError(errorText(e));
    }
  };
  void load();
  // While a link is out, watch for the chat to arrive; otherwise keep it fresh.
  const timer = setInterval(() => void load().then(() => setWaiting((w) => w && (status()?.chats.length ?? 0) === 0)), 3_000);
  onCleanup(() => clearInterval(timer));

  // The link is minted before it is needed, so the button is a plain link a
  // browser will follow: one opened after waiting on the server is a popup,
  // and a popup is blocked.
  const [invite, setInvite] = createSignal<{ url: string; code: string } | null>(null);
  const mint = async () => {
    try {
      const { url, code } = await api.telegramLink(clock());
      setInvite({ url, code });
    } catch {
      setInvite(null); // The bot may not have answered yet; the status line says so.
    }
  };

  const act = async (what: () => Promise<unknown>, done: string) => {
    setError(null);
    try {
      await what();
      setNote(done);
      await load();
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <>
      <Show when={status()} fallback={<div class="dim">{error() ?? 'Reading…'}</div>}>
        {(s) => (
          <Show when={s().enabled} fallback={<div class="dim">This server has no Telegram bot set up. Give it one with TRADOOR_TELEGRAM_TOKEN.</div>}>
            <For each={s().chats}>
              {(chat) => (
                <label class="swatch">
                  <span class="name">{chat.name ?? `Chat ${chat.chatId}`}</span>
                  <span class="num dim">linked {formatDateTime(chat.linkedAt)}</span>
                  <button class="dim" onClick={() => void act(() => api.telegramUnlink(chat.chatId), 'Disconnected.')}>
                    Disconnect
                  </button>
                </label>
              )}
            </For>
            <Show when={s().chats.length > 0}>
              <label class="swatch">
                <span class="name">Messages stay</span>
                <select
                  class="box wide"
                  data-tip="The bot takes its own messages back when their time is up, so the chat does not fill with old alerts. Telegram only allows this within 48 hours of sending."
                  onChange={(e) => void act(() => api.telegramClearing(e.currentTarget.value === 'keep' ? null : Number(e.currentTarget.value)), 'Saved.')}
                >
                  <For each={CLEARING}>
                    {(c) => (
                      <option value={c.value} selected={String(s().chats[0]?.clearAfterHours ?? 'keep') === c.value}>
                        {c.label}
                      </option>
                    )}
                  </For>
                </select>
              </label>
            </Show>
            <div style={{ display: 'flex', gap: '6px', 'align-items': 'center', 'margin-top': '6px', 'flex-wrap': 'wrap' }}>
              <Show when={invite()} fallback={<span class="dim">{s().bot ? 'Making a link…' : 'The bot has not answered yet.'}</span>}>
                {(link) => (
                  <>
                    <a class="btn" href={link().url} target="_blank" rel="noopener" onClick={() => setWaiting(true)}>
                      {s().chats.length > 0 ? 'Connect another chat' : 'Connect Telegram'}
                    </a>
                    <span class="dim num" data-tip="Or send the bot /start with this code">
                      /start {link().code}
                    </span>
                  </>
                )}
              </Show>
              <Show when={s().chats.length > 0}>
                <button class="btn" onClick={() => void act(() => api.telegramTest(), 'Test message sent.')}>
                  Send a test
                </button>
              </Show>
              <Show when={waiting()}>
                <span class="dim">waiting for Telegram…</span>
              </Show>
            </div>
            <div class="dim">
              {s().chats.length > 0
                ? `Alerts that fire are sent to these chats, whether or not a tab is open, written in ${offsetLabel(clock().offsetMinutes)}.`
                : 'Alerts fire on the server, so they reach Telegram with no browser open.'}
            </div>
          </Show>
        )}
      </Show>
      <Show when={error()}>{(e) => <div class="warn">▲ {e()}</div>}</Show>
      <Show when={note() && !error()}>{(n) => <div class="dim">{n()}</div>}</Show>
    </>
  );
}

export function Settings(props: { onClose(): void }) {
  return (
    <div class="backdrop" onPointerDown={() => props.onClose()}>
      <div
        class="dialog medium"
        tabIndex={-1}
        ref={(el) => queueMicrotask(() => el.focus())}
        onPointerDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key !== 'Escape') return;
          e.stopPropagation();
          props.onClose();
        }}
      >
        <header>
          <span class="title caps">Settings</span>
          <span class="spacer" />
          <button class="x" onClick={() => props.onClose()}>
            esc ✕
          </button>
        </header>
        <div class="form">
          <h3 class="caps">Browser id</h3>
          <OwnerRow />

          <h3 class="caps">Workspace</h3>
          <label class="swatch">
            <span class="name">Name</span>
            <input
              class="box wide"
              value={doc.name}
              maxLength={120}
              onChange={(e) => {
                const name = e.currentTarget.value.trim();
                if (name) update((d) => void (d.name = name));
              }}
            />
          </label>

          <h3 class="caps">Time</h3>
          <label class="swatch">
            <span class="name">Time zone</span>
            <select
              class="box wide"
              data-tip="Every time on the page — axis, crosshair, tape, clock. Daily and longer candles keep their exchange (UTC) dates."
              onChange={(e) => setThemeValue(TIME_ZONE_KEY, e.currentTarget.value === LOCAL_ZONE ? null : e.currentTarget.value)}
            >
              <For each={zoneChoices()}>
                {(z) => (
                  <option value={z.value} selected={timeZone() === z.value}>
                    {z.label}
                  </option>
                )}
              </For>
            </select>
          </label>

          <h3 class="caps">Telegram</h3>
          <TelegramRow />

          <h3 class="caps">Chart colours</h3>
          <div class="swatches">
            <For each={CHART_THEME}>{(t) => <Swatch name={t.name} label={t.label} fallback={t.value} />}</For>
          </div>
          <label class="swatch" style={{ 'margin-top': '6px' }}>
            <span class="name">Axis font size (px)</span>
            <input
              class="box num"
              type="number"
              min={8}
              max={20}
              value={doc.theme[FONT_PX.name] ?? FONT_PX.value}
              onChange={(e) => {
                const px = Number(e.currentTarget.value);
                setThemeValue(FONT_PX.name, px >= 8 && px <= 20 ? String(px) : null);
              }}
            />
          </label>

          <h3 class="caps">Page colours</h3>
          <div class="swatches">
            <For each={UI_THEME}>{(t) => <Swatch name={t.name} label={t.label} fallback={t.value} />}</For>
          </div>

          <h3 class="caps">Exchanges</h3>
          <div class="swatches">
            <For each={ALL_EXCHANGES}>
              {(x) => (
                <span class="swatch">
                  <ExchangeLogo exchange={x} size={16} />
                  <span class="name">{x.name}</span>
                  <span class={x.live ? 'bull' : 'dim'}>{x.live ? 'live' : 'coming later'}</span>
                </span>
              )}
            </For>
          </div>

          <div style={{ display: 'flex', gap: '6px', 'margin-top': '10px' }}>
            <button
              class="btn"
              onClick={() => {
                for (const name of Object.keys(doc.theme)) setThemeValue(name, null);
              }}
            >
              Reset colours
            </button>
            <button
              class="btn danger"
              onClick={() => {
                if (!confirm('Replace this workspace with the default layout?')) return;
                resetWorkspace();
                props.onClose();
              }}
            >
              Reset workspace
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

/** This browser's id, to copy — and another's, to take its drawings and layout over. */
function OwnerRow() {
  const [other, setOther] = createSignal('');
  const [copied, setCopied] = createSignal(false);
  return (
    <>
      <label class="swatch">
        <span class="name">Your drawings and layout belong to</span>
        <input class="box wide num" readOnly value={ownerId()} onFocus={(e) => e.currentTarget.select()} />
        <button
          class="btn"
          onClick={() => {
            void navigator.clipboard?.writeText(ownerId()).then(() => setCopied(true));
          }}
        >
          {copied() ? 'Copied' : 'Copy'}
        </button>
      </label>
      <label class="swatch">
        <span class="name">Use another browser's id</span>
        <input class="box wide num" value={other()} placeholder="paste the id from your other device" onInput={(e) => setOther(e.currentTarget.value)} />
        <button
          class="btn"
          disabled={!OWNER_PATTERN.test(other().trim())}
          onClick={() => {
            if (!confirm("Switch to that browser's drawings and layout? This browser's stay under its own id.")) return;
            // Taken over from the next load on: the page reads the id once.
            writePref(OWNER_KEY, other().trim());
            location.reload();
          }}
        >
          Use
        </button>
      </label>
      <p class="note" style={{ padding: '4px 0' }}>
        Drawings and layouts are kept on the server under this id, and only this browser sends it. A private window or another browser starts empty; paste the id there to carry
        yours over. Keep it to yourself — whoever has it can change them.
      </p>
    </>
  );
}
