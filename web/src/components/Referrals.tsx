/**
 * The exchanges tradoor charts, to open an account on through its referral
 * links: a dialog anyone can open from the top bar or the phone's menu, and the
 * card that offers them once (lib/referral). A link opens the exchange's own
 * page in a new tab, carrying the referral code and nothing about the visitor.
 */
import { For } from 'solid-js';
import { EXCHANGES, ExchangeLogo, type ExchangeId } from '../lib/exchanges';
import { Icon } from '../lib/icons';
import { dismissReferralCard } from '../lib/referral';

interface Partner {
  exchange: ExchangeId;
  url: string;
  offer: string;
}

const PARTNERS: readonly Partner[] = [
  { exchange: 'bybit', url: 'https://www.bybitglobal.com/invite?ref=P5RO8W&medium=referral&utm_campaign=evergreen&share_to=post', offer: 'Perpetuals · spot · copy trading' },
  { exchange: 'binance', url: 'https://www.bmwweb.biz/referral/earn-together/refer2earn-usdc/claim?hl=en&ref=GRO_28502_YJYU6&utm_source=referral_entrance', offer: 'Spot · futures · the deepest books' },
];

/** A link used: counted by the site's analytics, when it has them, and the card not offered again. */
function used(exchange: ExchangeId, from: 'dialog' | 'card'): void {
  (globalThis as { gtag?: (...args: unknown[]) => void }).gtag?.('event', 'referral_click', { exchange, from });
  dismissReferralCard();
}

export function ReferralDialog(props: { onClose(): void }) {
  return (
    <div class="backdrop" onPointerDown={() => props.onClose()}>
      <div
        class="dialog narrow ref"
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
          <span class="title caps">Partner exchanges</span>
          <span class="spacer" />
          <button class="x" onClick={() => props.onClose()}>
            esc ✕
          </button>
        </header>
        <p class="ref-intro">
          Open an account through these links and you get the exchange's new-user rewards. tradoor earns a share of the fees, at no extra cost to you — it keeps the charts free.
        </p>
        <div class="ref-list">
          <For each={PARTNERS}>
            {(p) => (
              <a class="ref-row" href={p.url} target="_blank" rel="sponsored noopener" onClick={() => used(p.exchange, 'dialog')}>
                <ExchangeLogo exchange={EXCHANGES[p.exchange]} size={28} />
                <span class="who">
                  <b>{EXCHANGES[p.exchange].name}</b>
                  <span>{p.offer}</span>
                </span>
                <span class="go">
                  Sign up <Icon name="external" size={11} stroke={2} />
                </span>
              </a>
            )}
          </For>
        </div>
        <footer>
          <span>Referral links · open in a new tab</span>
        </footer>
      </div>
    </div>
  );
}

export function ReferralCard() {
  return (
    <aside class="ref-card" aria-label="Partner exchanges">
      <header>
        <span class="eyebrow">
          <Icon name="gift" size={12} /> New to Bybit or Binance?
        </span>
        <button class="x" aria-label="Close" onClick={dismissReferralCard}>
          <Icon name="close" size={10} stroke={2.4} />
        </button>
      </header>
      <p>Sign up through tradoor's links for the new-user rewards — no extra cost, and it keeps the charts free.</p>
      <div class="pair">
        <For each={PARTNERS}>
          {(p) => (
            <a href={p.url} target="_blank" rel="sponsored noopener" onClick={() => used(p.exchange, 'card')}>
              <ExchangeLogo exchange={EXCHANGES[p.exchange]} size={18} />
              {EXCHANGES[p.exchange].name}
              <Icon name="external" size={10} stroke={2} />
            </a>
          )}
        </For>
      </div>
      <footer>
        <button class="later" onClick={dismissReferralCard}>
          Not now
        </button>
      </footer>
    </aside>
  );
}
