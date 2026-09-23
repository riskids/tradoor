import { createSignal, For, Show } from 'solid-js';
import { OWNER_KEY, OWNER_PATTERN, ownerId } from '../lib/owner';
import { ALL_EXCHANGES, ExchangeLogo } from '../lib/exchanges';
import { writePref } from '../lib/prefs';
import { deviceZone, LOCAL_ZONE, offsetLabel, offsetMinutes, TIME_ZONE_KEY, timeZone } from '../lib/timezone';
import { CHART_THEME, FONT_PX, UI_THEME } from '../lib/theme';
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
          title="Back to the default"
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
              title="Every time on the page — axis, crosshair, tape, clock. Daily and longer candles keep their exchange (UTC) dates."
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
