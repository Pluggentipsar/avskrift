// "Transkribera" as its own entry: menu, home card, highlighting, fresh start view.
// Run against `npm run dev`; mocked IPC with synthetic data only.
const { chromium } = require(process.env.AVSKRIFT_PLAYWRIGHT || 'playwright');
const fs = require('node:fs');
const assert = require('node:assert/strict');
const mocks = fs.readFileSync('node_modules/@tauri-apps/api/mocks.js', 'utf8').replace(/export \{[^}]+\};?/g, '');

(async () => {
  const browser = await chromium.launch({ headless: true, channel: 'msedge' });
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  const errors = []; page.on('pageerror', e => errors.push(e.message));
  await page.addInitScript(mocks + `
    const t = { language: 'sv', model: 'kb-whisper-small', diarized: false, utterances: [{ start: 0, end: 4, speaker: null, text: 'Hej och välkomna.', words: [] }] };
    const meeting = { version: 1, id: 'm1', jobType: 'meeting', title: 'Veckomöte', createdAt: '2026-10-02T08:00:00Z', updatedAt: '2026-10-02T08:00:00Z', transcript: t, category: '', actions: [], participants: [], enabled: [], rejected: [] };
    const file = { ...meeting, id: 'f1', jobType: 'transcribe', title: 'Intervju.mp3' };
    window.fixture = { jobs: [meeting, file], saves: 0 };
    mockWindows('main'); mockConvertFileSrc('windows');
    mockIPC(async (cmd, args) => {
      const f = window.fixture;
      switch (cmd) {
        case 'begin_work': return 'w'; case 'plugin:app|version': return 'test';
        case 'list_whisper_models': return [{ id: 'kb-whisper-small', label: 'Small', sizeMb: 1, downloaded: true }];
        case 'list_summary_models': return [{ id: 'qwen2.5-3b', label: 'Qwen', sizeMb: 1, downloaded: true }];
        case 'list_jobs': case 'search_jobs': return structuredClone(f.jobs);
        case 'open_job': return structuredClone(f.jobs.find(j => j.id === args.id));
        case 'save_job': f.saves++; return;
        case 'list_all_actions': case 'list_actions': case 'list_standalone_tasks': case 'list_summary_templates': return [];
        case 'dictation_snapshot': return { revision: 1, phase: 'idle', message: '', supported: true, backend: 'CPU', settings: { model: 'kb-whisper-small', saveHistory: false, enabled: true, autoInsert: true }, holdShortcutReady: true, toggleShortcutReady: true, entries: [] };
        default: return null;
      }
    }, { shouldMockEvents: true });`);
  const nav = () => page.getByRole('navigation', { name: 'Huvudnavigation' });
  const current = () => nav().locator('[aria-current=page]').innerText();
  const step = async (name, fn) => { await fn(); console.log(`ok  ${name}`); };
  try {
    await page.goto(process.env.AVSKRIFT_UI_URL || 'http://localhost:1420');
    await page.getByRole('heading', { name: 'Ditt arbete', exact: true }).waitFor();
    await step('menu has Transkribera right after Möten', async () => {
      const items = await nav().getByRole('button').allInnerTexts();
      assert.equal(items[items.indexOf('Möten') + 1], 'Transkribera');
    });
    await step('home card opens a fresh transcription view, marked Transkribera', async () => {
      await page.locator('.entrypoints').getByRole('button', { name: /Transkribera/ }).click();
      await page.getByRole('button', { name: 'Välj ljudfil…' }).waitFor();
      assert.equal(await current(), 'Transkribera');
      assert.equal(await page.locator('.workspace-location').innerText(), 'Transkribera');
    });
    await step('an open meeting is marked Möten', async () => {
      await nav().getByRole('button', { name: 'Ditt arbete' }).click();
      await page.getByRole('button', { name: /Veckomöte/ }).first().click();
      await page.getByRole('button', { name: 'Översikt' }).waitFor();
      assert.equal(await current(), 'Möten');
    });
    await step('Transkribera from an open meeting saves it and starts fresh', async () => {
      await nav().getByRole('button', { name: 'Transkribera' }).click();
      await page.getByRole('button', { name: 'Välj ljudfil…' }).waitFor();
      assert.equal(await current(), 'Transkribera');
      assert.equal(await page.getByRole('button', { name: 'Översikt' }).count(), 0);
    });
    await step('an open file transcription is marked Transkribera', async () => {
      await nav().getByRole('button', { name: 'Ditt arbete' }).click();
      await page.getByRole('button', { name: /Intervju\.mp3/ }).first().click();
      await page.getByRole('button', { name: 'Transkript', exact: true }).waitFor();
      assert.equal(await current(), 'Transkribera');
    });
    assert.deepEqual(errors, []);
    console.log('UI TRANSCRIBE ENTRY: all steps passed');
  } catch (e) {
    fs.mkdirSync('.build-tools', { recursive: true });
    await page.screenshot({ path: '.build-tools/transcribe-entry-failure.png' });
    console.error(e); console.error('page errors:', errors); process.exitCode = 1;
  } finally { await browser.close(); }
})();
