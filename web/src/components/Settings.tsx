import { createSignal, For, type JSX, onCleanup, Show } from 'solid-js';
import { api, errorText, type AccountStatus, type TelegramStatus } from '../lib/api';
import { freshOwnerId, OWNER_PATTERN, ownerId, switchOwner } from '../lib/owner';
import { ALL_EXCHANGES, ExchangeLogo } from '../lib/exchanges';
import { Icon, type IconName } from '../lib/icons';
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

/** The clock the reader reads the page in, which their Telegram messages follow. */
const clock = () => {
  const zone = timeZone() === LOCAL_ZONE ? deviceZone() : timeZone();
  return { zone, offsetMinutes: offsetMinutes(timeZone(), Date.now()) };
};

/** A labelled line of a card: what it is on the left, its control on the right. */
function Row(props: { label: string; tip?: string; children: JSX.Element }) {
  return (
    <div class="set-row">
      <span class="label" data-tip={props.tip}>
        {props.label}
      </span>
      <div class="grow">{props.children}</div>
    </div>
  );
}

/**
 * Signing in with Telegram: everything stays keyed by the browser id, and the
 * account is how a browser gets its id back. The link is minted before it is
 * needed, so the button is a plain link — one opened after waiting on the
 * server is a popup, and a popup is blocked — and while it is out the sign-in
 * is asked after until the reader says yes in the chat.
 */
function AccountSection() {
  const [status, setStatus] = createSignal<AccountStatus | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const [invite, setInvite] = createSignal<{ url: string; code: string } | null>(null);
  const [waiting, setWaiting] = createSignal(false);
  const [note, setNote] = createSignal<string | null>(null);

  const mint = async () => {
    try {
      setInvite(await api.signIn(clock()));
    } catch {
      setInvite(null); // The bot may not have answered yet; the card says so.
    }
  };
  const load = async () => {
    try {
      const next = await api.account();
      setStatus(next);
      setError(null);
      if (!next.account && next.telegram && next.bot && !invite()) void mint();
    } catch (e) {
      setError(errorText(e));
    }
  };
  void load();

  const ask = async () => {
    const code = invite()?.code;
    if (!code || !waiting()) return;
    try {
      const got = await api.signedIn(code);
      if (got.state === 'done') {
        setWaiting(false);
        // Another id: the account's things are under it, and the page loads them.
        if (got.owner !== ownerId()) switchOwner(got.owner);
        else void load();
      } else if (got.state === 'gone') {
        setWaiting(false);
        setNote('Not signed in: the link was declined or has expired. Try again.');
        setInvite(null);
        void mint();
      }
    } catch (e) {
      setError(errorText(e));
    }
  };
  const timer = setInterval(() => void ask(), 2_000);
  // A link is good for ten minutes: a fresh one before it lapses.
  const fresh = setInterval(() => {
    if (!waiting() && !status()?.account) void mint();
  }, 8 * 60_000);
  onCleanup(() => {
    clearInterval(timer);
    clearInterval(fresh);
  });

  const signOut = () => {
    if (!confirm('Sign out of this browser? Everything stays with your account; this browser starts empty until you sign in again.')) return;
    switchOwner(freshOwnerId());
  };

  return (
    <>
      <h2>Account</h2>
      <p class="lead">
        Your drawings, layout, watchlists and alerts are kept on the server. Sign in with Telegram and they follow you — after clearing this browser's history, or on
        another device, sign in again to have them back.
      </p>
      <Show when={status()} fallback={<div class="dim">{error() ?? 'Reading…'}</div>}>
        {(s) => (
          <Show
            when={s().account}
            fallback={
              <div class="set-card">
                <div class="set-account">
                  <span class="avatar out">
                    <Icon name="user" size={18} />
                  </span>
                  <div class="who">
                    <b>Not signed in</b>
                    <span class="dim">This browser keeps its things under its own id only.</span>
                  </div>
                </div>
                <Show when={s().telegram} fallback={<div class="dim">This server has no Telegram bot set up, so signing in is not on offer here.</div>}>
                  <div class="set-actions">
                    <Show when={invite()} fallback={<span class="dim">{s().bot ? 'Making a link…' : 'The bot has not answered yet.'}</span>}>
                      {(link) => (
                        <a
                          class="btn primary big"
                          href={link().url}
                          target="_blank"
                          rel="noopener"
                          onClick={() => {
                            setNote(null);
                            setWaiting(true);
                          }}
                        >
                          <Icon name="send" size={13} />
                          <span>Sign in with Telegram</span>
                        </a>
                      )}
                    </Show>
                    <Show when={waiting()}>
                      <span class="warn">Waiting — press “Yes, sign in” in the Telegram chat.</span>
                    </Show>
                  </div>
                  <p class="note">
                    No password and no phone number: the tradoor bot asks you to confirm in Telegram, and your alerts then come to that chat. If you already have an account,
                    this browser switches to it; anything made here before signing in stays behind.
                  </p>
                </Show>
              </div>
            }
          >
            {(a) => (
              <div class="set-card">
                <div class="set-account">
                  <span class="avatar">{(a().name ?? '?').replace(/^@/, '').slice(0, 1).toUpperCase()}</span>
                  <div class="who">
                    <b>{a().name ?? 'Telegram account'}</b>
                    <span class="dim">Signed in with Telegram · since {formatDateTime(a().since).slice(0, 10)}</span>
                  </div>
                  <button class="btn" onClick={signOut}>
                    Sign out of this browser
                  </button>
                </div>
                <p class="note">Sign in on any other browser or device to have the same drawings, layout, watchlists and alerts there.</p>
              </div>
            )}
          </Show>
        )}
      </Show>
      <Show when={error()}>{(e) => <div class="warn">▲ {e()}</div>}</Show>
      <Show when={note()}>{(n) => <div class="dim">{n()}</div>}</Show>
    </>
  );
}

/**
 * Alerts on Telegram. A bot cannot open a conversation, so the reader does:
 * the link carries a one-use code that tells the server which browser the chat
 * belongs to. While it is open the status is read again, so the chat appears
 * here the moment the bot has it.
 */
function TelegramSection() {
  const [status, setStatus] = createSignal<TelegramStatus | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const [note, setNote] = createSignal<string | null>(null);
  const [waiting, setWaiting] = createSignal(false);

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
      <h2>Telegram alerts</h2>
      <p class="lead">Price alerts and the signals you turn on fire on the server, so they reach these chats whether or not a tab is open.</p>
      <Show when={status()} fallback={<div class="dim">{error() ?? 'Reading…'}</div>}>
        {(s) => (
          <Show when={s().enabled} fallback={<div class="set-card dim">This server has no Telegram bot set up. Give it one with TRADOOR_TELEGRAM_TOKEN.</div>}>
            <div class="set-card">
              <h3>Chats</h3>
              <For each={s().chats} fallback={<div class="dim">No chat yet. Signing in with Telegram links yours, or connect one here.</div>}>
                {(chat) => (
                  <div class="set-row">
                    <Icon name="send" size={13} />
                    <span class="grow">{chat.name ?? `Chat ${chat.chatId}`}</span>
                    <span class="num dim">linked {formatDateTime(chat.linkedAt).slice(0, 16)}</span>
                    <button class="btn" onClick={() => void act(() => api.telegramUnlink(chat.chatId), 'Disconnected.')}>
                      Disconnect
                    </button>
                  </div>
                )}
              </For>
              <div class="set-actions">
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
                  <span class="warn">waiting for Telegram…</span>
                </Show>
              </div>
            </div>
            <Show when={s().chats.length > 0}>
              <div class="set-card">
                <h3>Messages</h3>
                <Row label="Messages stay" tip="The bot takes its own messages back when their time is up, so the chat does not fill with old alerts. Telegram only allows this within 48 hours of sending.">
                  <select class="box wide" onChange={(e) => void act(() => api.telegramClearing(e.currentTarget.value === 'keep' ? null : Number(e.currentTarget.value)), 'Saved.')}>
                    <For each={CLEARING}>
                      {(c) => (
                        <option value={c.value} selected={String(s().chats[0]?.clearAfterHours ?? 'keep') === c.value}>
                          {c.label}
                        </option>
                      )}
                    </For>
                  </select>
                </Row>
                <div class="dim">Times in them are written in {offsetLabel(clock().offsetMinutes)}, the zone set under General.</div>
              </div>
            </Show>
          </Show>
        )}
      </Show>
      <Show when={error()}>{(e) => <div class="warn">▲ {e()}</div>}</Show>
      <Show when={note() && !error()}>{(n) => <div class="dim">{n()}</div>}</Show>
    </>
  );
}

function GeneralSection() {
  return (
    <>
      <h2>General</h2>
      <p class="lead">The workspace's name and the clock every time on the page is written in.</p>
      <div class="set-card">
        <Row label="Workspace name">
          <input
            class="box wide"
            value={doc.name}
            maxLength={120}
            onChange={(e) => {
              const name = e.currentTarget.value.trim();
              if (name) update((d) => void (d.name = name));
            }}
          />
        </Row>
        <Row label="Time zone" tip="Every time on the page — axis, crosshair, tape, clock — and in Telegram messages. Daily and longer candles keep their exchange (UTC) dates.">
          <select class="box wide" onChange={(e) => setThemeValue(TIME_ZONE_KEY, e.currentTarget.value === LOCAL_ZONE ? null : e.currentTarget.value)}>
            <For each={zoneChoices()}>
              {(z) => (
                <option value={z.value} selected={timeZone() === z.value}>
                  {z.label}
                </option>
              )}
            </For>
          </select>
        </Row>
      </div>
    </>
  );
}

function ColoursSection() {
  return (
    <>
      <h2>Colours</h2>
      <p class="lead">Kept with the workspace, so they follow your account too. ↺ puts one back to the default.</p>
      <div class="set-card">
        <h3>Chart</h3>
        <div class="swatches">
          <For each={CHART_THEME}>{(t) => <Swatch name={t.name} label={t.label} fallback={t.value} />}</For>
        </div>
        <Row label="Axis font size (px)">
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
        </Row>
      </div>
      <div class="set-card">
        <h3>Page</h3>
        <div class="swatches">
          <For each={UI_THEME}>{(t) => <Swatch name={t.name} label={t.label} fallback={t.value} />}</For>
        </div>
      </div>
      <div class="set-actions">
        <button
          class="btn"
          onClick={() => {
            for (const name of Object.keys(doc.theme)) setThemeValue(name, null);
          }}
        >
          Reset all colours
        </button>
      </div>
    </>
  );
}

function AdvancedSection(props: { onClose(): void }) {
  return (
    <>
      <h2>Advanced</h2>
      <p class="lead">What signing in does for you, by hand, and the switches to start over.</p>
      <div class="set-card">
        <h3>Browser id</h3>
        <OwnerRows />
      </div>
      <div class="set-card">
        <h3>Exchanges</h3>
        <div class="swatches one">
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
      </div>
      <div class="set-card danger">
        <h3>Start over</h3>
        <div class="set-row">
          <span class="grow dim">Replace this workspace's panels and layout with the default. Drawings and alerts stay.</span>
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
    </>
  );
}

/** This browser's id, to copy — and another's, to take its drawings and layout over. */
function OwnerRows() {
  const [other, setOther] = createSignal('');
  const [copied, setCopied] = createSignal(false);
  return (
    <>
      <Row label="This browser's id">
        <div class="set-inline">
          <input class="box wide num" readOnly value={ownerId()} onFocus={(e) => e.currentTarget.select()} />
          <button class="btn" onClick={() => void navigator.clipboard?.writeText(ownerId()).then(() => setCopied(true))}>
            {copied() ? 'Copied' : 'Copy'}
          </button>
        </div>
      </Row>
      <Row label="Use another id">
        <div class="set-inline">
          <input class="box wide num" value={other()} placeholder="paste the id from your other device" onInput={(e) => setOther(e.currentTarget.value)} />
          <button
            class="btn"
            disabled={!OWNER_PATTERN.test(other().trim())}
            onClick={() => {
              if (confirm("Switch to that id's drawings and layout? This browser's stay under its own id.")) switchOwner(other().trim());
            }}
          >
            Use
          </button>
        </div>
      </Row>
      <p class="note">Everything is kept on the server under this id, and only this browser sends it. Keep it to yourself — whoever has it can change your things.</p>
    </>
  );
}

const SECTIONS: { id: string; label: string; icon: IconName }[] = [
  { id: 'account', label: 'Account', icon: 'user' },
  { id: 'telegram', label: 'Telegram', icon: 'send' },
  { id: 'general', label: 'General', icon: 'settings' },
  { id: 'colours', label: 'Colours', icon: 'droplet' },
  { id: 'advanced', label: 'Advanced', icon: 'command' },
];
/** The section open last, for the next time Settings opens. */
const [section, setSection] = createSignal('account');

export function Settings(props: { onClose(): void }) {
  return (
    <div class="backdrop" onPointerDown={() => props.onClose()}>
      <div
        class="dialog settings"
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
        <div class="set">
          <nav class="set-nav">
            <For each={SECTIONS}>
              {(s) => (
                <button classList={{ on: section() === s.id }} onClick={() => setSection(s.id)}>
                  <Icon name={s.icon} size={14} />
                  <span>{s.label}</span>
                </button>
              )}
            </For>
          </nav>
          <div class="set-body">
            <Show when={section() === 'account'}>
              <AccountSection />
            </Show>
            <Show when={section() === 'telegram'}>
              <TelegramSection />
            </Show>
            <Show when={section() === 'general'}>
              <GeneralSection />
            </Show>
            <Show when={section() === 'colours'}>
              <ColoursSection />
            </Show>
            <Show when={section() === 'advanced'}>
              <AdvancedSection onClose={props.onClose} />
            </Show>
          </div>
        </div>
      </div>
    </div>
  );
}
