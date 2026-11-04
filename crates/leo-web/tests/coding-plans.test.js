const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

function context() {
  const source = fs.readFileSync(path.resolve(__dirname, '../src/web/app/settings.js'), 'utf8');
  const value = { Date, rel: () => 'just now', esc: text => String(text).replace(/[<>&"]/g, ch => ({ '<': '&lt;', '>': '&gt;', '&': '&amp;', '"': '&quot;' }[ch])) };
  vm.createContext(value);
  vm.runInContext(source.slice(source.indexOf('const PLAN_STALE_MS'), source.indexOf('const THEME_LABELS')), value);
  return value;
}

test('coding accounts distinguish signed in, signed out, unknown and unavailable without changing the selection', () => {
  const value = context();
  for (const [installed, signed_in, label] of [[true, true, 'Signed in'], [true, false, 'Signed out'], [true, null, 'Sign-in unknown'], [false, null, 'Not available']]) {
    const page = { tasks: [{ task: 'writing', provider: 'ollama' }], plans: [{ provider: 'codex', name: 'Codex <img>', installed, signed_in, signin: 'codex login' }] };
    const html = value.codingPlans(page);
    assert.ok(html.includes(label));
    assert.equal(html.includes('data-action="use-plan"'), installed && signed_in === true);
    assert.ok(!html.includes('<img>'));
    assert.equal(page.tasks[0].provider, 'ollama');
    page.tasks[0].provider = 'codex';
    assert.ok(value.codingPlans(page).includes('Selected for writing'));
    assert.ok(!value.codingPlans(page).includes('data-action="use-plan"'));
  }
  assert.equal(value.codingPlans({ tasks: [] }), '');
});

test('plan meters handle both windows, limits, resets, saved readings and absent data', () => {
  const value = context();
  const now = Math.floor(Date.now() / 1000);
  const plan = { provider: 'codex', name: 'Codex', limits: { five_hour: { used: 0.25, resets_at: now + 3600 }, seven_day: { used: 0.93, resets_at: now + 86400 }, seen_at: new Date().toISOString() } };
  let html = value.planLimits(plan);
  assert.match(html, /aria-valuenow="25"/);
  assert.match(html, /aria-valuenow="93"/);
  assert.match(html, /near-limit/);
  assert.match(html, /Resets /);
  assert.ok(!html.includes('Saved usage'));
  plan.limits.seen_at = new Date(Date.now() - 600000).toISOString();
  assert.match(value.planLimits(plan), /Saved usage/);
  for (const [used, resets_at, percent, text] of [[2, null, 100, 'Reset time unavailable'], [-2, null, 0, 'Reset time unavailable'], [0.9, now - 1, 0, 'Waiting for the next update'], [0.2, 1e100, 20, 'Reset time unavailable']]) {
    plan.limits = { five_hour: { used, resets_at }, seen_at: 'bad-date' };
    html = value.planLimits(plan);
    assert.ok(html.includes(`aria-valuenow="${percent}"`));
    assert.ok(html.includes(text));
    assert.ok(!html.includes('Invalid Date'));
  }
  for (const limits of [null, {}, { five_hour: { used: 'bad' } }]) {
    assert.match(value.planLimits({ provider: 'codex', limits }), /Select Codex/);
    assert.match(value.planLimits({ provider: 'claude_code', limits }), /during an answer/);
  }
});
