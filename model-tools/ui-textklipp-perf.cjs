// Editor responsiveness on a long project (mocked IPC, same fixture format as ui-textklipp.cjs).
const { chromium } = require(process.env.AVSKRIFT_PLAYWRIGHT || 'playwright');
const fs = require('node:fs');
const mocks = fs.readFileSync('node_modules/@tauri-apps/api/mocks.js', 'utf8').replace(/export \{[^}]+\};?/g, '');
const project = fs.readFileSync(process.env.AVSKRIFT_TK_FIXTURE, 'utf8');
(async () => {
  const browser = await chromium.launch({ headless: true, channel: 'msedge' });
  const page = await browser.newPage({ viewport: { width: 1400, height: 900 } });
  await page.route('http://asset.localhost/**', r => r.fulfill({ status: 404, body: '' }));
  await page.addInitScript(mocks + `
    window.fixture = { project: ${project} };
    mockWindows('main'); mockConvertFileSrc('windows');
    mockIPC(async (cmd, args) => {
      const f = window.fixture;
      switch (cmd) {
        case 'begin_work': return 'w'; case 'plugin:app|version': return 'perf';
        case 'list_whisper_models': return [{ id: 'kb-whisper-large', label: 'L', sizeMb: 1, downloaded: true }];
        case 'list_jobs': case 'search_jobs': case 'list_all_actions': case 'list_actions': case 'list_standalone_tasks': case 'list_summary_models': case 'list_summary_templates': return [];
        case 'dictation_snapshot': return { revision: 1, phase: 'idle', message: '', supported: true, backend: 'CPU', settings: { model: 'kb-whisper-large', saveHistory: false, enabled: true, autoInsert: true }, holdShortcutReady: true, toggleShortcutReady: true, entries: [] };
        case 'wordalign_status': return { ready: true, available: true, sizeMb: 632 };
        case 'textklipp_list': return [{ id: f.project.id, title: 'Lång', updatedAt: f.project.updatedAt, duration: f.project.media.duration, status: 'ready', hasProxy: true }];
        case 'textklipp_open': return structuredClone(f.project);
        case 'textklipp_media': return { playback: null, isVideo: true };
        case 'textklipp_preview': return { keep: [[0, f.project.media.duration]], editedDuration: f.project.media.duration };
        case 'textklipp_save_edits': return 'x';
        default: return null;
      }
    }, { shouldMockEvents: true });`);
  await page.goto(process.env.AVSKRIFT_UI_URL || 'http://localhost:1420');
  await page.getByRole('navigation', { name: 'Huvudnavigation' }).getByRole('button', { name: 'Textklipp' }).click();
  let t = Date.now();
  await page.getByRole('button', { name: /Lång/ }).click();
  await page.locator('.doc [data-id]').first().waitFor();
  const spans = await page.locator('.doc [data-id]').count();
  console.log(`open: ${Date.now() - t} ms for ${spans} spans`);
  for (const [a, b] of [[100, 100], [2000, 2400], [4000, 4100]]) {
    await page.evaluate(([a, b]) => {
      const s = document.querySelector(`[data-id="${a}"]`), e = document.querySelector(`[data-id="${b}"]`);
      e.scrollIntoView(); const r = document.createRange(); r.setStart(s.firstChild, 0); r.setEnd(e.firstChild, e.firstChild.length);
      getSelection().removeAllRanges(); getSelection().addRange(r); window.__t = performance.now();
    }, [a, b]);
    await page.keyboard.press('Delete');
    await page.waitForFunction(id => document.querySelector(`[data-id="${id}"]`)?.classList.contains('struck'), b);
    console.log(`strike ${b - a + 1} words at ${a}: ${Math.round(await page.evaluate(() => performance.now() - window.__t))} ms`);
  }
  await browser.close();
})();
