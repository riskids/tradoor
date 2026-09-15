import { For } from 'solid-js';

const GROUPS: { title: string; keys: [string, string][] }[] = [
  {
    title: 'Workspace',
    keys: [
      ['/  Ctrl K', 'Commands'],
      ['?', 'This help'],
      ['drag a tab', 'Dock it: centre or edge of a group'],
      ['link icon on a tab', 'Link group — panels share a market'],
      ['middle-click tab', 'Close the panel'],
    ],
  },
  {
    title: 'Chart',
    keys: [
      ['a–z', 'Search for a symbol'],
      ['←  →', 'Shorter / longer timeframe'],
      ['wheel', 'Zoom time'],
      ['drag', 'Pan time; up or down moves price'],
      ['wheel or drag price axis', 'Scale price'],
      ['AUTO', 'Price follows the bars again'],
      ['double-click price axis', 'Auto scale that pane'],
      ['right-click', 'Chart menu'],
    ],
  },
  {
    title: 'Drawing',
    keys: [
      ['esc', 'Cancel the tool, or deselect'],
      ['Delete', 'Remove the selected drawing'],
      ['Enter', 'Finish a path'],
      ['double-click', 'Finish a path'],
    ],
  },
  {
    title: 'Symbol search',
    keys: [
      ['↑  ↓', 'Move'],
      ['↵', 'Open'],
      ['tab', 'All / perp / spot'],
      ['bybit btc', 'Venue and kind words filter'],
      ['esc', 'Close'],
    ],
  },
];

export function Help(props: { onClose(): void }) {
  return (
    <div class="backdrop" onPointerDown={() => props.onClose()}>
      <div
        class="dialog"
        tabIndex={-1}
        ref={(el) => queueMicrotask(() => el.focus())}
        onPointerDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key !== 'Escape' && e.key !== '?') return;
          e.stopPropagation();
          props.onClose();
        }}
      >
        <header>
          <span class="title caps">Keyboard</span>
          <span class="spacer" />
          <button class="x" onClick={() => props.onClose()}>
            esc ✕
          </button>
        </header>
        <div class="keys">
          <For each={GROUPS}>
            {(g) => (
              <div>
                <h3 class="caps primary" style={{ margin: '0 0 6px', 'font-weight': '400' }}>
                  {g.title}
                </h3>
                <dl>
                  <For each={g.keys}>
                    {([key, label]) => (
                      <div>
                        <dd>{label}</dd>
                        <dt>
                          <kbd>{key}</kbd>
                        </dt>
                      </div>
                    )}
                  </For>
                </dl>
              </div>
            )}
          </For>
        </div>
      </div>
    </div>
  );
}
