import { createSignal, For, Show, type JSX } from 'solid-js';
import { Icon, type IconName } from '../lib/icons';
import { useDismiss } from '../lib/dismiss';
import { setOverlay } from '../app/ui';
import { doc, activeLayoutId, autosave, layouts, openLayout, refreshLayouts, saveNow, saveState, setAutosave, setStarred } from './store';

const MOD = /Mac|iPhone|iPad/.test(navigator.platform) ? '⌘' : 'Ctrl';
const RECENT = 5;

/** Topbar button + dropdown for saving, copying, creating and opening layouts. */
export function LayoutsMenu() {
  const [open, setOpen] = createSignal(false);
  useDismiss(open, () => setOpen(false));
  const recent = () => [...layouts()].sort((a, b) => (b.openedAt ?? b.updatedAt ?? 0) - (a.openedAt ?? a.updatedAt ?? 0)).slice(0, RECENT);
  const run = (fn: () => void) => () => (setOpen(false), fn());
  const go = (id: string, e: MouseEvent | PointerEvent, aux = false) => {
    setOpen(false);
    openLayout(id, aux || e.metaKey || e.ctrlKey);
  };
  const Item = (p: { icon?: IconName; label: string; hint?: string; dim?: boolean; onClick(): void; children?: JSX.Element }) => (
    <button role="menuitem" classList={{ dim: p.dim }} onClick={p.onClick}>
      <span class="lay-label">
        <span class="lay-ico">{p.icon && <Icon name={p.icon} size={13} />}</span>
        {p.label}
      </span>
      {p.children ?? <Show when={p.hint}><kbd>{p.hint}</kbd></Show>}
    </button>
  );
  return (
    <div class="lay-wrap" onPointerDown={(e) => e.stopPropagation()}>
      <button
        class="item"
        classList={{ on: open() }}
        aria-haspopup="menu"
        aria-expanded={open()}
        data-tip="Chart layouts"
        onClick={() => (setOpen(!open()), open() && void refreshLayouts())}
      >
        <span class="lay-name">{doc.name}</span>
        <Icon name="chevron" size={11} />
      </button>
      <Show when={open()}>
        <div class="menu lay-menu" role="menu">
          <Item label="Save layout" hint={`${MOD} S`} dim={saveState() === 'saved'} onClick={run(() => void saveNow())} />
          <button role="menuitemcheckbox" aria-checked={autosave()} onClick={() => setAutosave(!autosave())}>
            <span class="lay-label"><span class="lay-ico" />Autosave</span>
            <span class="switch" classList={{ on: autosave() }} />
          </button>
          <Item icon="copy" label="Make a copy…" onClick={run(() => setOverlay({ kind: 'layoutName', mode: 'copy' }))} />
          <Item icon="pencil" label="Rename…" onClick={run(() => setOverlay({ kind: 'layoutName', mode: 'rename' }))} />
          <hr />
          <Item icon="plus" label="Create new layout…" onClick={run(() => setOverlay({ kind: 'layoutName', mode: 'create' }))} />
          <hr />
          <div class="caps lay-head">Recently used</div>
          <For each={recent()}>
            {(l) => (
              <div class="lay-row" classList={{ cur: l.id === activeLayoutId(), on: l.starred }}>
                <button role="menuitem" onClick={(e) => go(l.id, e)} onAuxClick={(e) => e.button === 1 && go(l.id, e, true)} onMouseDown={(e) => e.button === 1 && e.preventDefault()}>
                  <span class="lay-text">
                    <span class="lay-title">{l.name}</span>
                    <span class="lay-sub">{l.summary}</span>
                  </span>
                </button>
                <button class="lay-star" classList={{ on: l.starred }} aria-label={l.starred ? 'Unstar layout' : 'Star layout'} aria-pressed={l.starred} onClick={() => void setStarred(l.id, !l.starred)}>
                  <Icon name="star" size={13} fill={l.starred} />
                </button>
              </div>
            )}
          </For>
          <hr />
          <Item label="Open layout…" hint="." onClick={run(() => setOverlay({ kind: 'layouts' }))} />
        </div>
      </Show>
    </div>
  );
}
