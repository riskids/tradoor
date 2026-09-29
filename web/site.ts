/**
 * What the built page tells search engines and browsers about the site:
 * absolute URLs for its canonical address and preview image, robots.txt and a
 * sitemap — all from `SITE_URL` (`https://example.com`), set where the image
 * is built — and a Content Security Policy.
 *
 * Without `SITE_URL` the page still builds: the preview image stays relative
 * and robots.txt names no sitemap.
 *
 * `GA_ID` (`G-XXXXXXXXXX`), set the same way, adds Google Analytics: Google's
 * loader and a short start-up script of our own, which the policy allows by
 * name. Without it the page carries no analytics and the policy stays as tight.
 */
import type { Plugin } from 'vite';

declare const process: { env: Record<string, string | undefined> };

/**
 * Scripts only from this origin (and Google's tag host, with analytics),
 * WebAssembly compiled, data from the exchanges over TLS. Styles inline because Solid sets them on elements. The page may
 * not be framed (nginx's `frame-ancestors`; a meta tag cannot say it).
 */
export function csp(analytics = false): string {
  // Google's loader comes from its tag host and falls back to an image when a
  // beacon cannot be sent; the beacons themselves are already https.
  const google = analytics ? ' https://www.googletagmanager.com' : '';
  const pixels = analytics ? ' https://*.google-analytics.com https://*.googletagmanager.com' : '';
  return [
    "default-src 'self'",
    `script-src 'self' 'wasm-unsafe-eval'${google}`,
    "worker-src 'self'",
    "style-src 'self' 'unsafe-inline'",
    `img-src 'self' data: blob:${pixels}`,
    "font-src 'self' data:",
    "connect-src 'self' https: wss:",
    "manifest-src 'self'",
    "object-src 'none'",
    "base-uri 'self'",
    "form-action 'self'",
  ].join('; ');
}

/** A GA4 measurement id, or empty; anything else is a mistake worth a failed build. */
export function analyticsId(raw: string | undefined): string {
  const id = (raw ?? '').trim().toUpperCase();
  if (id && !/^G-[A-Z0-9]{4,16}$/.test(id)) throw new Error(`GA_ID must be a GA4 measurement id, like G-ABC123DEF4 — got ${raw}`);
  return id;
}

/** What gtag.js expects to find: a queue, and the id to count page views for.
 *  A file of our own rather than Google's inline snippet, which the policy
 *  would refuse. */
export function analyticsScript(id: string): string {
  return [
    'window.dataLayer = window.dataLayer || [];',
    'function gtag() { dataLayer.push(arguments); }',
    "gtag('js', new Date());",
    `gtag('config', '${id}');`,
    '',
  ].join('\n');
}

export function siteUrl(raw: string | undefined): string {
  const url = (raw ?? '').trim().replace(/\/+$/, '');
  if (url && !/^https:\/\/[a-z0-9.-]+(:\d+)?$/i.test(url)) throw new Error(`SITE_URL must be the site's https origin, like https://example.com — got ${raw}`);
  return url;
}

export function site(): Plugin {
  const url = siteUrl(process.env.SITE_URL);
  const ga = analyticsId(process.env.GA_ID);
  return {
    name: 'tradoor-site',
    transformIndexHtml: {
      order: 'post',
      handler(html, ctx) {
        // Development runs Vite's own client, which a policy would have to allow,
        // and its visits are nobody's to count.
        if (ctx.server) return html;
        let out = html.replace('<meta charset="UTF-8" />', `<meta charset="UTF-8" />\n    <meta http-equiv="Content-Security-Policy" content="${csp(!!ga)}" />`);
        if (ga) {
          out = out.replace('</head>', `  <script defer src="/ga.js"></script>\n    <script async src="https://www.googletagmanager.com/gtag/js?id=${ga}"></script>\n  </head>`);
        }
        if (url) {
          out = out
            .replaceAll('content="/og.png"', `content="${url}/og.png"`)
            .replace('<meta property="og:type"', `<link rel="canonical" href="${url}/" />\n    <meta property="og:url" content="${url}/" />\n    <meta property="og:type"`);
        }
        return out;
      },
    },
    generateBundle() {
      const robots = ['User-agent: *', 'Allow: /', 'Disallow: /api/', 'Disallow: /data/', ...(url ? ['', `Sitemap: ${url}/sitemap.xml`] : [])].join('\n');
      this.emitFile({ type: 'asset', fileName: 'robots.txt', source: `${robots}\n` });
      if (ga) this.emitFile({ type: 'asset', fileName: 'ga.js', source: analyticsScript(ga) });
      if (url) {
        const sitemap = `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n  <url><loc>${url}/</loc><changefreq>weekly</changefreq><priority>1.0</priority></url>\n</urlset>\n`;
        this.emitFile({ type: 'asset', fileName: 'sitemap.xml', source: sitemap });
      }
    },
  };
}
