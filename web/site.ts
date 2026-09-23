/**
 * What the built page tells search engines and browsers about the site:
 * absolute URLs for its canonical address and preview image, robots.txt and a
 * sitemap — all from `SITE_URL` (`https://example.com`), set where the image
 * is built — and a Content Security Policy.
 *
 * Without `SITE_URL` the page still builds: the preview image stays relative
 * and robots.txt names no sitemap.
 */
import type { Plugin } from 'vite';

declare const process: { env: Record<string, string | undefined> };

/**
 * Scripts only from this origin, WebAssembly compiled, data from the exchanges
 * over TLS. Styles inline because Solid sets them on elements. The page may
 * not be framed (nginx's `frame-ancestors`; a meta tag cannot say it).
 */
export const CSP = [
  "default-src 'self'",
  "script-src 'self' 'wasm-unsafe-eval'",
  "worker-src 'self'",
  "style-src 'self' 'unsafe-inline'",
  "img-src 'self' data: blob:",
  "font-src 'self' data:",
  "connect-src 'self' https: wss:",
  "manifest-src 'self'",
  "object-src 'none'",
  "base-uri 'self'",
  "form-action 'self'",
].join('; ');

export function siteUrl(raw: string | undefined): string {
  const url = (raw ?? '').trim().replace(/\/+$/, '');
  if (url && !/^https:\/\/[a-z0-9.-]+(:\d+)?$/i.test(url)) throw new Error(`SITE_URL must be the site's https origin, like https://example.com — got ${raw}`);
  return url;
}

export function site(): Plugin {
  const url = siteUrl(process.env.SITE_URL);
  return {
    name: 'tradoor-site',
    transformIndexHtml: {
      order: 'post',
      handler(html, ctx) {
        // Development runs Vite's own client, which a policy would have to allow.
        if (ctx.server) return html;
        let out = html.replace('<meta charset="UTF-8" />', `<meta charset="UTF-8" />\n    <meta http-equiv="Content-Security-Policy" content="${CSP}" />`);
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
      if (url) {
        const sitemap = `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n  <url><loc>${url}/</loc><changefreq>weekly</changefreq><priority>1.0</priority></url>\n</urlset>\n`;
        this.emitFile({ type: 'asset', fileName: 'sitemap.xml', source: sitemap });
      }
    },
  };
}
