import { createEffect, createMemo, createSignal, For, on, onMount } from 'solid-js';

export interface Command {
  group: string;
  label: string;
  hint?: string;
  run(): void;
}

/** Everything the app can do, by name. Every word typed must appear. */
export function CommandPalette(props: { commands: Command[]; onClose(): void }) {
  const [query, setQuery] = createSignal('');
  const [cursor, setCursor] = createSignal(0);
  let input!: HTMLInputElement;
  let list!: HTMLDivElement;
  onMount(() => input.focus());

  const matches = createMemo(() => {
    const words = query().trim().toLowerCase().split(/\s+/).filter(Boolean);
    if (words.length === 0) return props.commands;
    return props.commands.filter((c) => {
      const text = `${c.group} ${c.label} ${c.hint ?? ''}`.toLowerCase();
      return words.every((w) => text.includes(w));
    });
  });
  createEffect(on(query, () => setCursor(0), { defer: true }));
  createEffect(() => {
    cursor();
    list.querySelector('[data-cur]')?.scrollIntoView({ block: 'nearest' });
  });

  const run = (c: Command | undefined) => {
    if (!c) return;
    // Closed first, so a command can open another dialog.
    props.onClose();
    c.run();
  };

  return (
    <div class="backdrop" onPointerDown={() => props.onClose()}>
      <div
        class="dialog medium"
        onPointerDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          const n = matches().length;
          if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
            e.preventDefault();
            if (n > 0) setCursor((c) => (c + (e.key === 'ArrowDown' ? 1 : -1) + n) % n);
          } else if (e.key === 'Enter') {
            e.preventDefault();
            run(matches()[cursor()]);
          } else if (e.key === 'Escape') {
            e.preventDefault();
            e.stopPropagation();
            props.onClose();
          }
        }}
      >
        <header>
          <span class="title caps">Command</span>
          <input ref={input} class="q" value={query()} onInput={(e) => setQuery(e.currentTarget.value)} placeholder="new chart, 4 hours, log scale, link blue…" />
        </header>
        <div ref={list} class="list">
          <For each={matches()} fallback={<div class="empty">No command matches “{query()}”.</div>}>
            {(c, i) => (
              <div class="row" classList={{ cur: i() === cursor() }} data-cur={i() === cursor() ? '' : undefined} onMouseMove={() => setCursor(i())} onClick={() => run(c)}>
                <span class="dim" style={{ 'min-width': '90px' }}>
                  {c.group}
                </span>
                <span class="grow">{c.label}</span>
                {c.hint ? <kbd>{c.hint}</kbd> : null}
              </div>
            )}
          </For>
        </div>
        <footer>
          <span>{matches().length} commands</span>
          <span>↑↓ move · ↵ run · esc close</span>
        </footer>
      </div>
    </div>
  );
}
