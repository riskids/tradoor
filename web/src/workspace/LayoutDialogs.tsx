import { createMemo, createSignal, For, onMount, Show } from 'solid-js';
import { Icon } from '../lib/icons';
import { formatAge } from '../lib/format';
import { doc, activeLayoutId, copyLayout, createLayout, deleteLayout, layouts, openLayout, refreshLayouts, renameLayout, setStarred } from './store';

const stopKeys = (e: KeyboardEvent, close: () => void) => {
  if (e.key !== 'Escape') return false;
  e.preventDefault();
  e.stopPropagation();
  close();
  return true;
};

/** Every saved layout: search, open, star, delete. Newest edit first. */
export function OpenLayoutDialog(props: { onClose(): void }) {
  const [query, setQuery] = createSignal('');
  onMount(() => void refreshLayouts());
  const rows = createMemo(() => {
    const q = query().trim().toLowerCase();
    return layouts()
      .filter((l) => !q || l.name.toLowerCase().includes(q) || l.summary.toLowerCase().includes(q))
      .sort((a, b) => (b.updatedAt ?? 0) - (a.updatedAt ?? 0));
  });
  const open = (id: string, e: { metaKey: boolean; ctrlKey: boolean }) => {
    props.onClose();
    openLayout(id, e.metaKey || e.ctrlKey);
  };
  return (
    <div class="backdrop" onPointerDown={() => props.onClose()}>
      <div class="dialog medium" onPointerDown={(e) => e.stopPropagation()}>
        <header>
          <span class="title caps">Open layout</span>
          <span class="spacer" />
          <button class="x" aria-label="Close" onClick={() => props.onClose()}>
            <Icon name="close" size={12} />
          </button>
        </header>
        <div class="lay-search">
          <Icon name="search" size={13} />
          <input
            class="q"
            placeholder="Search"
            ref={(el) => setTimeout(() => el.focus())}
            value={query()}
            onInput={(e) => setQuery(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (stopKeys(e, props.onClose)) return;
              const first = rows()[0];
              if (e.key === 'Enter' && first) open(first.id, e);
            }}
          />
        </div>
        <div class="list">
          <For each={rows()} fallback={<div class="lay-empty">No layouts found</div>}>
            {(l) => (
              <div class="row lay-drow" classList={{ cur: l.id === activeLayoutId() }} onClick={(e) => open(l.id, e)} onAuxClick={(e) => e.button === 1 && open(l.id, { metaKey: true, ctrlKey: false })}>
                <span class="lay-text grow">
                  <span class="lay-title">{l.name}</span>
                  <span class="lay-sub">{l.summary}</span>
                </span>
                <span class="dim">{l.updatedAt ? formatAge(l.updatedAt) : ''}</span>
                <button class="lay-star" classList={{ on: l.starred }} aria-label={l.starred ? 'Unstar layout' : 'Star layout'} aria-pressed={l.starred} onClick={(e) => (e.stopPropagation(), void setStarred(l.id, !l.starred))}>
                  <Icon name="star" size={13} fill={l.starred} />
                </button>
                {/* ponytail: the open layout has no delete; add a "switch first" flow if that's wanted. */}
                <button
                  class="lay-del"
                  aria-label="Delete layout"
                  data-tip="Delete layout"
                  disabled={l.id === activeLayoutId()}
                  onClick={(e) => {
                    e.stopPropagation();
                    if (confirm(`Delete layout "${l.name}"? This can't be undone.`)) void deleteLayout(l.id);
                  }}
                >
                  <Icon name="trash" size={13} />
                </button>
              </div>
            )}
          </For>
        </div>
        <footer>
          <span>↵ open first · {/Mac/.test(navigator.platform) ? '⌘' : 'Ctrl'}-click new window · esc close</span>
        </footer>
      </div>
    </div>
  );
}

const TITLES = { create: 'Create new layout', copy: 'Make a copy', rename: 'Rename' } as const;

/** Name prompt shared by Create new layout, Make a copy and Rename. */
export function LayoutNameDialog(props: { mode: 'create' | 'copy' | 'rename'; onClose(): void }) {
  const [text, setText] = createSignal(props.mode === 'copy' ? `${doc.name} copy` : props.mode === 'rename' ? doc.name : '');
  const [busy, setBusy] = createSignal(false);
  let input!: HTMLInputElement;
  onMount(() => (input.focus(), input.select()));
  const submit = async () => {
    const name = text().trim();
    if (!name || busy()) return;
    setBusy(true);
    try {
      await (props.mode === 'create' ? createLayout : props.mode === 'copy' ? copyLayout : renameLayout)(name);
      props.onClose();
    } finally {
      setBusy(false);
    }
  };
  return (
    <div class="backdrop" style={{ 'padding-top': '20vh' }} onPointerDown={() => props.onClose()}>
      <form
        class="dialog narrow"
        role="dialog"
        aria-label={TITLES[props.mode]}
        onPointerDown={(e) => e.stopPropagation()}
        onSubmit={(e) => (e.preventDefault(), void submit())}
      >
        <header>
          <span class="title caps">{TITLES[props.mode]}</span>
        </header>
        <input
          ref={input}
          class="q"
          style={{ padding: '8px' }}
          maxLength={80}
          placeholder="Layout name"
          value={text()}
          onInput={(e) => setText(e.currentTarget.value)}
          onKeyDown={(e) => void stopKeys(e, props.onClose)}
        />
        <footer>
          <span>↵ save · esc cancel</span>
          <span>
            <button class="btn" type="button" onClick={() => props.onClose()}>Cancel</button>{' '}
            <button class="btn on" type="submit" disabled={text().trim() === '' || busy()}>Save</button>
          </span>
        </footer>
      </form>
    </div>
  );
}
