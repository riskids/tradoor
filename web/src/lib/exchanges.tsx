/**
 * Exchanges and their marks. Binance and Bybit are live; the rest are listed so
 * their logos and names are ready when their venues, streams and history
 * loaders are added — flip `live` then, and they appear in search and filters.
 * Marks are the official ones, on a black disc, from rsalianto's exchange icons.
 */
import type { Component, JSX } from 'solid-js';
import { Dynamic, For } from 'solid-js/web';
import { MARKS, type ExchangeMark } from './exchange-marks';
import { parseMarket, type Venue } from './markets';

export type ExchangeId = 'binance' | 'bybit' | 'okx' | 'bitget' | 'gate' | 'htx' | 'bitmex' | 'bitfinex' | 'aster' | 'lighter';

interface LogoProps {
  size?: number;
}

export interface ExchangeDef {
  id: ExchangeId;
  name: string;
  /** Venues this app can chart, stream and store. */
  live: boolean;
  Logo: Component<LogoProps>;
}

function Mark(props: LogoProps & { viewBox: string; children: JSX.Element }) {
  const size = () => props.size ?? 14;
  return (
    <svg class="xlogo" width={size()} height={size()} viewBox={props.viewBox} aria-hidden="true">
      {props.children}
    </svg>
  );
}

/** A mark whose paths the chart engine draws too — one source of artwork. */
function Paths(props: LogoProps & { mark: ExchangeMark }) {
  return (
    <Mark size={props.size} viewBox={`0 0 ${props.mark.box} ${props.mark.box}`}>
      <For each={props.mark.paths}>{(path) => <path d={path.d} fill={path.fill} transform={path.t ? `translate(${path.t[0]} ${path.t[1]}) scale(${path.t[2]})` : undefined} />}</For>
    </Mark>
  );
}

const Binance: Component<LogoProps> = (p) => <Paths size={p.size} mark={MARKS.binance!} />;

const Bybit: Component<LogoProps> = (p) => <Paths size={p.size} mark={MARKS.bybit!} />;

const Okx: Component<LogoProps> = (p) => (
  <Mark size={p.size} viewBox="0 0 32 32">
    <circle cx="16" cy="16" r="16" fill="#000" />
    <g transform="translate(2 2) scale(1.16667)" fill="#FFF">
      <path d="M3 3h6v6H3zm12 6H9v6H3v6h6v-6h6v6h6v-6h-6zm0 0V3h6v6z" />
    </g>
  </Mark>
);

const Bitget: Component<LogoProps> = (p) => (
  <Mark size={p.size} viewBox="0 0 32 32">
    <circle cx="16" cy="16" r="16" fill="#000" />
    <g transform="translate(2 2) scale(1.16667)" fill="#00F0FF">
      <path d="M11.121 9.46h4.283l4.381 4.555a.785.785 0 0 1 .003 1.076L14.17 21H9.757l1.334-1.357l4.898-5.092l-4.836-5.092" />
      <path d="M12.879 14.54H8.596L4.215 9.986a.785.785 0 0 1-.003-1.076L9.83 3h4.412l-1.334 1.357L8.01 9.449l4.836 5.092" />
    </g>
  </Mark>
);

const Gate: Component<LogoProps> = (p) => (
  <Mark size={p.size} viewBox="0 0 32 32">
    <circle cx="16" cy="16" r="16" fill="#000" />
    <g transform="translate(5 5) scale(0.036667)">
      <path fill="#2354E6" d="M300 465a165 165 0 0 1 0-330V0a300 300 0 1 0 300 300H465a165 165 0 0 1-165 165" />
      <path fill="#17E6A1" d="M300 300h165V135H300z" />
    </g>
  </Mark>
);

const Htx: Component<LogoProps> = (p) => (
  <Mark size={p.size} viewBox="0 0 16 16">
    <circle cx="8" cy="8" r="8" fill="#000" />
    <g transform="translate(1.2 1.2) scale(0.85)">
      <path d="M9.8 4.92A6.73 6.73 0 0 0 7.83 0h-.06C7.7 4.5 5.43 5.7 4.2 7.32a5.43 5.43 0 0 0 2.52 8.59c1.52.42-.35-.75-.6-3.23-.3-3 3.7-5.28 3.7-7.76z" fill="#1b2143" />
      <path d="M11.1 6.43h-.06A8.17 8.17 0 0 1 9.7 9.29c-2.45 3.72-1 5.5-.26 6.48.45.56 0 0 1.13-.57a5.16 5.16 0 0 0 2.46-4 5.77 5.77 0 0 0-1.93-4.8z" fill="#2ca6e0" />
    </g>
  </Mark>
);

const Bitmex: Component<LogoProps> = (p) => (
  <Mark size={p.size} viewBox="0 0 16 16">
    <circle cx="8" cy="8" r="8" fill="#000" />
    <g transform="translate(1.2 1.2) scale(0.85)">
      <path d="M5.5 3h6.05a.42.42 0 0 1-.08.22L9.46 6.88h-3.7a3.39 3.39 0 0 1-.2.38l-.26.45c-.1.2-.22.37-.3.56s-.38.67-.57 1l-.58 1a3 3 0 0 0-.22.39l-1.2 2.1a.67.67 0 0 1-.13.21h-2A1 1 0 0 0 0 13a1.08 1.08 0 0 1 .21-.49c.1-.22.24-.4.34-.63Q1.78 9.7 3 7.5c.2-.3.34-.64.53-1l.7-1.23c.07-.17.18-.33.26-.5l.9-1.6A.79.79 0 0 1 5.51 3z" fill="#eb4a60" />
      <path d="M12.52 5.14l1.2-2.12h2.2L16 3l-.2.34-1.57 2.88-.65 1.16v.05l-3.07 5.53h-6c.1-.24.25-.47.37-.7L6.6 9.1h3.7l2.27-4z" fill="#595fb5" />
    </g>
  </Mark>
);

const Bitfinex: Component<LogoProps> = (p) => (
  <Mark size={p.size} viewBox="0 0 16 16">
    <circle cx="8" cy="8" r="8" fill="#000" />
    <g transform="translate(1.2 1.2) scale(0.85)">
      <path d="M.6 13.73c.73.8 4.8 4.57 11.15.2 4.6-3.62 4.48-11.46 4-13.83C15.64.43 10.3 12.25.6 13.73z" fill="#97c554" />
      <path d="M15.8.1A18.59 18.59 0 0 0 4 3.06a8.49 8.49 0 0 0-4 8.12C8.53 10.22 15.6.36 15.8.1z" fill="#719b37" />
    </g>
  </Mark>
);

const Aster: Component<LogoProps> = (p) => (
  <Mark size={p.size} viewBox="0 0 32 32">
    <defs>
      <linearGradient id="xlogo-aster" x1="18.94" x2="12.64" y1="0" y2="32.05" gradientUnits="userSpaceOnUse">
        <stop stop-color="#F4D5B1" />
        <stop offset="1" stop-color="#FFD29F" />
      </linearGradient>
    </defs>
    <circle cx="16" cy="16" r="16" fill="#000" />
    <g transform="translate(6.1 6.1) scale(0.62)" fill="url(#xlogo-aster)">
      <path d="m9.13 30.44.75-3.45a6.19 6.19 0 0 0-6.06-7.5H.4a16.04 16.04 0 0 0 8.74 10.95Z" />
      <path d="M10.64 31.07A16.08 16.08 0 0 0 31.73 19.5H21.25c-4.73 0-8.81 3.28-9.81 7.88l-.8 3.69Z" />
      <path d="M32 17.9A16.03 16.03 0 0 0 17.37.05l-2.24 10.36a6.19 6.19 0 0 0 6.06 7.49H32Z" />
      <path d="M15.75 0A16.03 16.03 0 0 0 .1 17.9h3.65c4.73 0 8.81-3.28 9.81-7.89L15.75 0Z" />
    </g>
  </Mark>
);

const Lighter: Component<LogoProps> = (p) => (
  <Mark size={p.size} viewBox="0 0 32 32">
    <circle cx="16" cy="16" r="16" fill="#000" />
    <path fill="#FFF" transform="translate(10.6 5.9) scale(0.78)" d="M5.989 20.388 0 25.498V5.888L5.989.502v19.886Zm7.811.012-5.989 5.098V17.85l5.989-5.37v7.92Z" />
  </Mark>
);

export const EXCHANGES: Record<ExchangeId, ExchangeDef> = {
  binance: { id: 'binance', name: 'Binance', live: true, Logo: Binance },
  bybit: { id: 'bybit', name: 'Bybit', live: true, Logo: Bybit },
  okx: { id: 'okx', name: 'OKX', live: false, Logo: Okx },
  bitget: { id: 'bitget', name: 'Bitget', live: false, Logo: Bitget },
  gate: { id: 'gate', name: 'Gate', live: false, Logo: Gate },
  htx: { id: 'htx', name: 'HTX', live: false, Logo: Htx },
  bitmex: { id: 'bitmex', name: 'BitMEX', live: false, Logo: Bitmex },
  bitfinex: { id: 'bitfinex', name: 'Bitfinex', live: false, Logo: Bitfinex },
  aster: { id: 'aster', name: 'Aster', live: false, Logo: Aster },
  lighter: { id: 'lighter', name: 'Lighter', live: false, Logo: Lighter },
};

export const ALL_EXCHANGES: readonly ExchangeDef[] = Object.values(EXCHANGES);
export const LIVE_EXCHANGES: readonly ExchangeDef[] = ALL_EXCHANGES.filter((e) => e.live);

export const exchangeOfVenue = (venue: Venue | string): ExchangeDef => EXCHANGES[venue.split('-')[0] as ExchangeId] ?? EXCHANGES.binance;

export function ExchangeLogo(props: { exchange: ExchangeDef; size?: number }) {
  return <Dynamic component={props.exchange.Logo} size={props.size} />;
}

/** Logo, optionally the exchange's name, and PERP or SPOT. */
export function VenueTag(props: { market: string; name?: boolean; size?: number }) {
  const ref = () => parseMarket(props.market);
  const exchange = () => exchangeOfVenue(ref()?.venue ?? 'binance');
  return (
    <span class="venue" title={ref() ? `${exchange().name} ${ref()!.venue.endsWith('perp') ? 'perpetual' : 'spot'}` : undefined}>
      <ExchangeLogo exchange={exchange()} size={props.size} />
      {props.name ? <span class="venue-name">{exchange().name}</span> : null}
      <span class="kind">{ref()?.venue.endsWith('perp') ? 'PERP' : 'SPOT'}</span>
    </span>
  );
}
