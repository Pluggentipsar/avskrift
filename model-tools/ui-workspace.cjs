// Workspace layout: one primary action in the title row, rare views under "Mer", a player bar
// pinned to the bottom, and a rail with summary, decisions and actions beside the transcript.
// Run against `npm run dev`; mocked IPC with synthetic data only.
const { chromium } = require(process.env.AVSKRIFT_PLAYWRIGHT || 'playwright');
const fs = require('node:fs');
const assert = require('node:assert/strict');
const mocks = fs.readFileSync('node_modules/@tauri-apps/api/mocks.js', 'utf8').replace(/export \{[^}]+\};?/g, '');

(async () => {
  const browser = await chromium.launch({ headless: true, channel: 'msedge' });
  const page = await browser.newPage({ viewport: { width: 1440, height: 800 } });
  const errors = []; page.on('pageerror', e => errors.push(e.message));
  await page.addInitScript(mocks + `
    const utterances = Array.from({ length: 60 }, (_, i) => ({ start: i * 5, end: i * 5 + 4, speaker: null, text: 'Avsnitt ' + i + ' om budgeten och schemat.', words: [] }));
    const meeting = { version: 1, id: 'm1', jobType: 'meeting', title: 'Veckomöte', createdAt: '2026-10-02T08:00:00Z', updatedAt: '2026-10-02T08:00:00Z',
      transcript: { language: 'sv', model: 'kb-whisper-small', diarized: false, utterances }, audioPath: 'C:/x/m1.wav', lastView: 'transcript',
      summaryDraft: '## Beslut\\n- **Budgeten** ligger kvar.', decisions: [{ id: 'd1', text: 'Budgeten ligger kvar.', start: 120 }],
      actions: [{ id: 'a1', text: 'Skicka protokollet', done: false, assignee: 'Anna', due: '' }], category: '', participants: [], enabled: [], rejected: [] };
    window.fixture = { jobs: [meeting], saved: [] };
    mockWindows('main'); mockConvertFileSrc('windows');
    mockIPC(async (cmd, args) => {
      const f = window.fixture;
      switch (cmd) {
        case 'begin_work': return 'w'; case 'plugin:app|version': return 'test';
        case 'list_whisper_models': return [{ id: 'kb-whisper-small', label: 'Small', sizeMb: 1, downloaded: true }];
        case 'list_summary_models': return [{ id: 'qwen2.5-3b', label: 'Qwen', sizeMb: 1, downloaded: true }];
        case 'list_jobs': case 'search_jobs': return structuredClone(f.jobs);
        case 'open_job': return structuredClone(f.jobs.find(j => j.id === args.id));
        case 'save_job': f.saved.push(structuredClone(args.job ?? args)); return;
        case 'list_all_actions': case 'list_actions': case 'list_standalone_tasks': case 'list_summary_templates': return [];
        case 'dictation_snapshot': return { revision: 1, phase: 'idle', message: '', supported: true, backend: 'CPU', settings: { model: 'kb-whisper-small', saveHistory: false, enabled: true, autoInsert: true }, holdShortcutReady: true, toggleShortcutReady: true, entries: [] };
        default: return null;
      }
    }, { shouldMockEvents: true });`);
  // A silent 5-minute WAV so the player has a duration and can seek.
  const rate = 8000, n = rate * 300, wav = Buffer.alloc(44 + n, 128);
  wav.write('RIFF', 0); wav.writeUInt32LE(36 + n, 4); wav.write('WAVEfmt ', 8); wav.writeUInt32LE(16, 16); wav.writeUInt16LE(1, 20); wav.writeUInt16LE(1, 22);
  wav.writeUInt32LE(rate, 24); wav.writeUInt32LE(rate, 28); wav.writeUInt16LE(1, 32); wav.writeUInt16LE(8, 34); wav.write('data', 36); wav.writeUInt32LE(n, 40);
  // Answer byte ranges like the real asset protocol; without them the media isn't seekable.
  await page.route(/asset\.localhost/, route => {
    const m = /bytes=(\d+)-(\d*)/.exec(route.request().headers().range || '');
    if (!m) return route.fulfill({ status: 200, contentType: 'audio/wav', headers: { 'accept-ranges': 'bytes' }, body: wav });
    const a = Number(m[1]), z = m[2] ? Math.min(Number(m[2]), wav.length - 1) : wav.length - 1;
    return route.fulfill({ status: 206, contentType: 'audio/wav', headers: { 'accept-ranges': 'bytes', 'content-range': `bytes ${a}-${z}/${wav.length}` }, body: wav.subarray(a, z + 1) });
  });
  const tabs = () => page.getByRole('navigation', { name: 'Vyer i aktuellt arbete' });
  const rail = () => page.getByRole('complementary', { name: 'Sammanfattning, beslut och åtgärder' });
  const step = async (name, fn) => { await fn(); console.log(`ok  ${name}`); };
  try {
    await page.goto(process.env.AVSKRIFT_UI_URL || 'http://localhost:1420');
    await page.getByRole('heading', { name: 'Ditt arbete', exact: true }).waitFor();
    await page.getByRole('button', { name: /Veckomöte/ }).first().click();
    await tabs().getByRole('button', { name: 'Transkript', exact: true }).click();
    await rail().waitFor();
    await step('title row has one primary action; the rest sits in the ⋯ menu', async () => {
      const header = page.locator('.workspace-header');
      assert.equal(await header.locator('.btn.primary').count(), 1);
      assert.equal(await header.locator('.btn.primary').innerText(), 'Exportera…');
      assert.equal(await page.getByRole('button', { name: 'Original och versioner' }).isVisible(), false);
      await page.getByLabel('Fler åtgärder').click();
      await page.getByRole('button', { name: 'Original och versioner' }).waitFor();
      await page.getByRole('button', { name: 'Kopiera för AI' }).waitFor();
      await page.getByLabel('Fler åtgärder').click();
    });
    await step('rare views live under Mer, and the open one names the menu', async () => {
      assert.equal(await tabs().getByRole('button', { name: 'Fråga källan' }).isVisible(), false);
      await page.getByLabel('Fler vyer').click();
      await tabs().getByRole('button', { name: 'Fråga källan' }).click();
      assert.equal(await tabs().getByRole('button', { name: 'Fråga källan' }).isVisible(), false, 'menu closes after a choice');
      assert.equal(await page.getByLabel('Fler vyer').innerText(), 'Fråga källan');
      await tabs().getByRole('button', { name: 'Transkript', exact: true }).click();
      assert.equal(await page.getByLabel('Fler vyer').innerText(), 'Mer');
    });
    await step('rail shows a plain summary, decisions that seek, and actions that save', async () => {
      await rail().getByText('• Budgeten ligger kvar.').waitFor();
      await rail().getByRole('button', { name: /Budgeten ligger kvar/ }).click();
      await page.waitForFunction(() => Math.round(Number(document.querySelector('.seek')?.value)) === 120);
      await page.getByRole('button', { name: 'Pausa' }).click();
      const before = await page.evaluate(() => fixture.saved.length);
      await rail().getByRole('checkbox', { name: /Skicka protokollet/ }).check();
      await page.waitForFunction(n => fixture.saved.length > n, before);
    });
    await step('player bar stays at the bottom while the page scrolls', async () => {
      await page.locator('main.review').evaluate(e => e.scrollTo(0, e.scrollHeight));
      await page.locator('main.review').evaluate(e => e.scrollTo(0, 0));
      const box = await page.locator('.review > .player').boundingBox();
      assert.ok(Math.abs(box.y + box.height - 800) <= 1, `player bottom at ${box.y + box.height}`);
    });
    await step('narrow window drops the rail, keeps the transcript', async () => {
      await page.setViewportSize({ width: 800, height: 768 });
      await page.waitForTimeout(100);
      assert.equal(await rail().isVisible(), false);
      assert.ok(await page.getByRole('region', { name: 'Transkriptets avsnitt' }).isVisible());
    });
    assert.deepEqual(errors, []);
    console.log('UI WORKSPACE: all steps passed');
  } catch (e) {
    fs.mkdirSync('.build-tools', { recursive: true });
    await page.screenshot({ path: '.build-tools/workspace-failure.png' });
    console.error(e); console.error('page errors:', errors); process.exitCode = 1;
  } finally { await browser.close(); }
})();
