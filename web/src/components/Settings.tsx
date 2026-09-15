import { createSignal, For, Show } from 'solid-js';
import { API_TOKEN_PREF } from '../lib/api';
import { ALL_EXCHANGES, ExchangeLogo } from '../lib/exchanges';
import { readPref, writePref } from '../lib/prefs';
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

export function Settings(props: { onClose(): void }) {
  const [token, setToken] = createSignal(readPref(API_TOKEN_PREF) ?? '');
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

          <h3 class="caps">Server</h3>
          <label class="swatch">
            <span class="name">API token for saving drawings and layouts</span>
            <input
              class="box wide"
              type="password"
              autocomplete="off"
              value={token()}
              placeholder="only when the server sets TRADOOR_API_TOKEN"
              onChange={(e) => {
                setToken(e.currentTarget.value);
                writePref(API_TOKEN_PREF, e.currentTarget.value.trim());
              }}
            />
          </label>
          <p class="note" style={{ padding: '4px 0' }}>
            Kept in this browser only; it is never part of the saved workspace.
          </p>

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
