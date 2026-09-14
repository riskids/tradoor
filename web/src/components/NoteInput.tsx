import { createSignal, onMount } from 'solid-js';

/** The text of a note whose anchor is already placed. Cancelling drops it —
 *  a note nobody wrote is not a drawing. */
export function NoteInput(props: { onSave(label: string): void; onClose(): void }) {
  const [text, setText] = createSignal('');
  let input!: HTMLInputElement;
  onMount(() => input.focus());
  const save = () => {
    const label = text().trim();
    props.onClose();
    if (label) props.onSave(label);
  };
  return (
    <div class="backdrop" style={{ 'padding-top': '20vh' }} onPointerDown={() => props.onClose()}>
      <form
        class="dialog narrow"
        onPointerDown={(e) => e.stopPropagation()}
        onSubmit={(e) => {
          e.preventDefault();
          save();
        }}
      >
        <header>
          <span class="title caps">Note</span>
        </header>
        <input
          ref={input}
          class="q"
          style={{ padding: '8px' }}
          maxLength={500}
          value={text()}
          onInput={(e) => setText(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key !== 'Escape') return;
            e.preventDefault();
            e.stopPropagation();
            props.onClose();
          }}
          placeholder="what this bar is about…"
        />
        <footer>
          <span>↵ save · esc cancel</span>
          <button class="btn" type="submit" disabled={text().trim() === ''}>
            Save
          </button>
        </footer>
      </form>
    </div>
  );
}
