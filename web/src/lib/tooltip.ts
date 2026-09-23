/**
 * The app's own tooltips, in place of the browser's `title`: any element with
 * `data-tip` shows it after a short rest, placed below it — or above, near the
 * bottom of the window — and kept on screen. Once one is showing, the next
 * comes at once, as moving along a toolbar expects. Keyboard focus shows it
 * too. One element serves every tooltip; nothing is added per element.
 */

const DELAY_MS = 400;
/** After a tooltip, the next one this soon shows without waiting. */
const WARM_MS = 600;
const GAP = 6;
const MARGIN = 6;

/**
 * `title` also named an icon-only button for screen readers; a tooltip does
 * not. Such elements take their tooltip as `aria-label` as they appear.
 */
function label(root: ParentNode): void {
  const els = root instanceof Element && root.matches('[data-tip]') ? [root] : [];
  for (const el of [...els, ...root.querySelectorAll('[data-tip]')]) {
    const tip = el.getAttribute('data-tip');
    if (tip && !el.textContent?.trim()) el.setAttribute('aria-label', tip);
  }
}

export function installTooltips(): void {
  label(document.body);
  new MutationObserver((records) => {
    for (const r of records) {
      if (r.type === 'attributes') label(r.target as Element);
      else for (const node of r.addedNodes) if (node.nodeType === 1) label(node as Element);
    }
  }).observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ['data-tip'] });

  const tip = document.createElement('div');
  tip.className = 'tip';
  tip.setAttribute('role', 'tooltip');
  document.body.append(tip);

  let target: HTMLElement | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let hiddenAt = 0;

  const place = (el: HTMLElement) => {
    const r = el.getBoundingClientRect();
    const w = tip.offsetWidth;
    const h = tip.offsetHeight;
    const left = Math.min(Math.max(MARGIN, r.left + r.width / 2 - w / 2), innerWidth - w - MARGIN);
    const below = r.bottom + GAP;
    const top = below + h > innerHeight - MARGIN ? Math.max(MARGIN, r.top - GAP - h) : below;
    tip.style.transform = `translate(${Math.round(left)}px, ${Math.round(top)}px)`;
  };

  const show = (el: HTMLElement) => {
    const text = el.dataset.tip;
    if (!text || !el.isConnected) return;
    tip.textContent = text;
    tip.classList.add('show');
    place(el);
  };

  const hide = () => {
    clearTimeout(timer);
    if (tip.classList.contains('show')) hiddenAt = Date.now();
    tip.classList.remove('show');
    target = null;
  };

  const tipped = (node: EventTarget | null): HTMLElement | null => (node instanceof Element ? node.closest<HTMLElement>('[data-tip]') : null);

  document.addEventListener('pointerover', (e) => {
    if (e.pointerType === 'touch') return;
    const el = tipped(e.target);
    if (el === target) return;
    hide();
    if (!el) return;
    target = el;
    const warm = Date.now() - hiddenAt < WARM_MS;
    timer = setTimeout(() => target === el && show(el), warm ? 0 : DELAY_MS);
  });
  document.addEventListener('pointerout', (e) => {
    if (target && !target.contains(e.relatedTarget as Node | null)) hide();
  });
  document.addEventListener('focusin', (e) => {
    const el = tipped(e.target);
    if (el && el === e.target && el.matches(':focus-visible')) {
      hide();
      target = el;
      show(el);
    }
  });
  document.addEventListener('focusout', hide);
  for (const kind of ['pointerdown', 'keydown', 'wheel'] as const) document.addEventListener(kind, hide, { capture: true, passive: true });
  addEventListener('scroll', hide, { capture: true, passive: true });
  addEventListener('blur', hide);
}
