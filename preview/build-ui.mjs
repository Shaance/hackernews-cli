import { mkdir, readFile, writeFile, copyFile, rm } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
const root = new URL('.', import.meta.url),
  out = new URL('./dist/', root);
const backend = new URL(process.env.PREVIEW_BACKEND_URL || 'https://hn-session.hashcode.dev');
if (!['https:', 'http:'].includes(backend.protocol) || backend.href !== backend.origin + '/')
  throw Error('PREVIEW_BACKEND_URL must be an HTTP(S) origin');
let project;
try {
  project = await readFile(new URL('.vercel/project.json', out));
} catch (error) {
  if (error.code !== 'ENOENT') throw error;
}
await rm(out, { recursive: true, force: true });
await mkdir(out, { recursive: true });
if (project) {
  await mkdir(new URL('.vercel/', out));
  await writeFile(new URL('.vercel/project.json', out), project);
}
const html = (await readFile(new URL('index.html', root), 'utf8')).replace(
  'name="preview-backend" content=""',
  `name="preview-backend" content="${backend.origin}"`,
);
await writeFile(new URL('index.html', out), html);
for (const [source, target] of [
  ['favicon.svg', 'favicon.svg'],
  ['app.js', 'app.js'],
  ['style.css', 'style.css'],
  ['node_modules/@xterm/xterm/lib/xterm.js', 'xterm.js'],
  ['node_modules/@xterm/addon-fit/lib/addon-fit.js', 'fit.js'],
  ['node_modules/@xterm/xterm/css/xterm.css', 'xterm.css'],
])
  await copyFile(new URL(source, root), new URL(target, out));
const websocket = backend.origin.replace(/^http/, 'ws');
await writeFile(
  new URL('vercel.json', out),
  JSON.stringify(
    {
      headers: [
        {
          source: '/(.*)',
          headers: [
            {
              key: 'Content-Security-Policy',
              value: `default-src 'self'; connect-src 'self' ${backend.origin} ${websocket}; style-src 'self' 'unsafe-inline'; script-src 'self'; img-src 'self'; frame-src https: http:; object-src 'none'; frame-ancestors 'none'; base-uri 'none'`,
            },
            { key: 'X-Content-Type-Options', value: 'nosniff' },
            { key: 'Cache-Control', value: 'no-cache' },
          ],
        },
      ],
    },
    null,
    2,
  ),
);
console.log(`Static UI built at ${fileURLToPath(out)}; backend ${backend.origin}`);
