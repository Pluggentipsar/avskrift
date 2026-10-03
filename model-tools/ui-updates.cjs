// Update dialog: checks only on request, follows the chosen channel, installs from an installed copy,
// points a portable copy to GitHub. Mocked IPC; nothing is downloaded.
const { chromium } = require(process.env.AVSKRIFT_PLAYWRIGHT || 'playwright');
const fs = require('node:fs');
const assert = require('node:assert/strict');
const mocks = fs.readFileSync('node_modules/@tauri-apps/api/mocks.js', 'utf8').replace(/export \{[^}]+\};?/g, '');

(async () => {
  const browser = await chromium.launch({ headless: true, channel: 'msedge' });
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  const errors = []; page.on('pageerror', e => errors.push(e.message));
  await page.addInitScript(mocks + `
    window.fixture = { calls: [], installed: true, available: null };
    mockWindows('main'); mockConvertFileSrc('windows');
    mockIPC(async (cmd, args) => {
      const f = window.fixture;
      switch (cmd) {
        case 'begin_work': return 'w'; case 'plugin:app|version': return '0.8.0-beta.3';
        case 'list_whisper_models': return [{ id: 'kb-whisper-small', label: 'Small', sizeMb: 1, downloaded: true }];
        case 'list_summary_models': return [{ id: 'qwen2.5-3b', label: 'Qwen', sizeMb: 1, downloaded: true }];
        case 'list_jobs': case 'search_jobs': case 'list_all_actions': case 'list_actions': case 'list_standalone_tasks': case 'list_summary_templates': return [];
        case 'dictation_snapshot': return { revision: 1, phase: 'idle', message: '', supported: true, backend: 'CPU', settings: { model: 'kb-whisper-small', saveHistory: false, enabled: true, autoInsert: true }, holdShortcutReady: true, toggleShortcutReady: true, entries: [] };
        case 'check_update': f.calls.push(['check', args.channel]); return { current: '0.8.0-beta.3', installed: f.installed, variant: 'vulkan', available: f.available };
        case 'install_update': f.calls.push(['install', args.channel]);
          await window.__TAURI_INTERNALS__.invoke('plugin:event|emit', { event: 'avskrift:update-progress', payload: { received: 50 * 1024 * 1024, total: 200 * 1024 * 1024 } });
          return new Promise(() => {}); // the real installer closes the app
        default: return null;
      }
    }, { shouldMockEvents: true });`);
  const nav = () => page.getByRole('navigation', { name: 'Huvudnavigation' });
  const dialog = () => page.getByRole('dialog', { name: 'Uppdateringar' });
  const step = async (name, fn) => { await fn(); console.log(`ok  ${name}`); };
  try {
    await page.goto(process.env.AVSKRIFT_UI_URL || 'http://localhost:1420');
    await page.getByRole('heading', { name: 'Ditt arbete', exact: true }).waitFor();
    await step('nothing is checked until asked; a pre-release follows pre-releases', async () => {
      await page.getByRole('button', { name: 'Sök efter uppdatering', exact: true }).click();
      await dialog().waitFor();
      assert.deepEqual(await page.evaluate(() => fixture.calls), []);
      assert.ok(await dialog().getByLabel('Även förhandsversioner').isChecked());
    });
    await step('up to date', async () => {
      await dialog().getByRole('button', { name: 'Sök efter uppdatering' }).click();
      await dialog().getByText('Du har den senaste versionen.').waitFor();
      assert.deepEqual(await page.evaluate(() => fixture.calls), [['check', 'beta']]);
    });
    await step('the chosen channel is used and remembered', async () => {
      await dialog().getByLabel('Stabila versioner').check();
      await page.evaluate(() => { fixture.available = { version: '0.9.0', notes: 'Nytt: bättre möten.' }; });
      await dialog().getByRole('button', { name: 'Sök efter uppdatering' }).click();
      await dialog().getByText('Version 0.9.0 finns').waitFor();
      await dialog().getByText('Nytt: bättre möten.').waitFor();
      assert.deepEqual((await page.evaluate(() => fixture.calls)).at(-1), ['check', 'stable']);
      assert.equal(await page.evaluate(() => localStorage.getItem('avskrift.updateChannel')), 'stable');
    });
    await step('an installed copy downloads with progress', async () => {
      await dialog().getByRole('button', { name: 'Ladda ner och installera' }).click();
      await dialog().getByText('Hämtar 50 av 200 MB…').waitFor();
      assert.deepEqual((await page.evaluate(() => fixture.calls)).at(-1), ['install', 'stable']);
    });
    await step('a portable copy is pointed to GitHub instead', async () => {
      await page.reload();
      await page.getByRole('heading', { name: 'Ditt arbete', exact: true }).waitFor();
      await page.evaluate(() => { fixture.installed = false; fixture.available = { version: '0.9.0', notes: null }; });
      await page.getByRole('button', { name: 'Sök efter uppdatering', exact: true }).click();
      assert.ok(await dialog().getByLabel('Stabila versioner').isChecked(), 'channel remembered');
      await dialog().getByRole('button', { name: 'Sök efter uppdatering' }).click();
      await dialog().getByRole('button', { name: 'Öppna releasen på GitHub' }).waitFor();
      assert.equal(await dialog().getByRole('button', { name: 'Ladda ner och installera' }).count(), 0);
    });
    assert.deepEqual(errors, []);
    console.log('UI UPDATES: all steps passed');
  } catch (e) {
    fs.mkdirSync('.build-tools', { recursive: true });
    await page.screenshot({ path: '.build-tools/updates-failure.png' });
    console.error(e); console.error('page errors:', errors); process.exitCode = 1;
  } finally { await browser.close(); }
})();
