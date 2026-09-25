import { For, Show } from 'solid-js';
import { Icon } from '../lib/icons';
import { dismiss, toasts } from '../lib/toasts';

export function Toasts() {
  return (
    <div class="toasts" aria-live="polite">
      <For each={toasts()}>
        {(t) => (
          <div
            class={`toast ${t.tone}`}
            classList={{ clickable: !!t.action }}
            onClick={() => {
              t.action?.();
              dismiss(t.id);
            }}
          >
            <span class="icon">
              <Icon name={t.tone === 'alert' ? 'alarm' : 'info'} size={14} />
            </span>
            <span class="text">
              <b>
                <Show when={t.way !== null && t.way !== undefined}>
                  <span class={t.way ? 'bull' : 'bear'}>{t.way ? '▲' : '▼'} </span>
                </Show>
                {t.title}
              </b>
              {t.body ? <span>{t.body}</span> : null}
            </span>
            <button
              class="x"
              aria-label="Dismiss"
              onClick={(e) => {
                e.stopPropagation();
                dismiss(t.id);
              }}
            >
              <Icon name="close" size={10} stroke={2.4} />
            </button>
          </div>
        )}
      </For>
    </div>
  );
}
