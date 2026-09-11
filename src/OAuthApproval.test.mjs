import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';

// Execute the shipped inline script, rendering Rust format! escapes and its random id slot.
const source = readFileSync(new URL('../src-tauri/src/oauth/http.rs', import.meta.url), 'utf8');
const script = source.match(/<script nonce="\{nonce\}">([\s\S]*?)<\/script>/)[1]
  .replace('/oauth/approval/{}', '/oauth/approval/test-id')
  .replaceAll('{{', '{').replaceAll('}}', '}');

function page(fetch) {
  const status = { textContent: '' };
  const timers = [];
  const redirects = [];
  let now = 0;
  runInNewContext(script, {
    document: { getElementById: () => status },
    Date: { now: () => now },
    fetch,
    location: { replace: (url) => redirects.push(url) },
    setTimeout: (callback) => timers.push(callback),
  });
  return { status, timers, redirects, tick: () => timers.shift()(), expire: () => { now = 120001; } };
}

test('approval polling requests JSON through ngrok, then redirects after native approval', async () => {
  let approved = false;
  const app = page(async (url, options) => {
    assert.equal(url, '/oauth/approval/test-id');
    assert.equal(options.credentials, 'omit');
    assert.equal(options.cache, 'no-store');
    // Model the observed ngrok behavior: browser traffic without this header gets HTML 200.
    if (options.headers['ngrok-skip-browser-warning'] !== '1') return new Response('<html>ERR_NGROK_6024</html>', { headers: { 'content-type': 'text/html' } });
    assert.equal(options.headers.Accept, 'application/json');
    return Response.json(approved ? { status: 'approved', redirect: 'https://client.example/cb?code=test-code' } : { status: 'pending' });
  });
  await app.tick();
  assert.equal(app.timers.length, 1);
  assert.deepEqual(app.redirects, []);
  approved = true;
  await app.tick();
  assert.deepEqual(app.redirects, ['https://client.example/cb?code=test-code']);
  assert.equal(app.timers.length, 0);
});

test('HTML 200 from a gateway stops with an actionable error and never redirects', async () => {
  const app = page(async () => new Response('<html>gateway login</html>', { headers: { 'content-type': 'text/html' } }));
  await app.tick();
  assert.match(app.status.textContent, /网关返回了非 JSON 页面/);
  assert.equal(app.timers.length, 0);
  assert.deepEqual(app.redirects, []);
});

test('network interruptions retry within the existing authorization deadline', async () => {
  const app = page(async () => { throw new TypeError('network failure'); });
  await app.tick();
  assert.match(app.status.textContent, /连接暂时中断/);
  assert.equal(app.timers.length, 1);
  app.expire();
  await app.tick();
  assert.match(app.status.textContent, /授权已过期/);
  assert.equal(app.timers.length, 0);
  assert.deepEqual(app.redirects, []);
});

test('native denial redirects to the client and expired requests stop', async () => {
  for (const result of [{ status: 'denied', redirect: 'https://client.example/cb?error=access_denied' }, { status: 'expired' }]) {
    const app = page(async () => Response.json(result));
    await app.tick();
    assert.deepEqual(app.redirects, result.redirect ? [result.redirect] : []);
    assert.equal(app.timers.length, 0);
  }
});
