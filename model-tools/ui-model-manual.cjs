// Manual model install: links to download in the browser, then picked files are handed to the app.
// Mocked IPC; nothing is downloaded.
const { chromium } = require(process.env.AVSKRIFT_PLAYWRIGHT || 'playwright');
const fs = require('node:fs');
const assert = require('node:assert/strict');
const mocks = fs.readFileSync('node_modules/@tauri-apps/api/mocks.js', 'utf8').replace(/export \{[^}]+\};?/g, '');

(async () => {
  const browser = await chromium.launch({ headless: true, channel: 'msedge' });
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  const errors = []; page.on('pageerror', e => errors.push(e.message));
  await page.addInitScript(mocks + `
    window.fixture = { calls: [], downloaded: false, missing: ['https://h/kb/ggml-model-q5_0.bin'] };
    mockWindows('main'); mockConvertFileSrc('windows');
    mockIPC(async (cmd, args) => {
      const f = window.fixture;
      switch (cmd) {
        case 'begin_work': return 'w'; case 'plugin:app|version': return '0.8.0-beta.4';
        case 'list_whisper_models': return [{ id: 'kb-whisper-small', label: 'KB Small', sizeMb: 488, downloaded: f.downloaded }];
        case 'list_summary_models': return [{ id: 'qwen2.5-3b', label: 'Qwen', sizeMb: 1, downloaded: true }];
        case 'list_jobs': case 'search_jobs': case 'list_all_actions': case 'list_actions': case 'list_standalone_tasks': case 'list_summary_templates': return [];
        case 'wordalign_status': return { ready: true, available: true, sizeMb: 632 };
        case 'runtime_memory_status': return { busy: false, text: null, speech: null, memory: null };
        case 'dictation_snapshot': return { revision: 1, phase: 'idle', message: '', supported: true, backend: 'CPU', settings: { model: 'kb-whisper-small', saveHistory: false, enabled: true, autoInsert: true }, holdShortcutReady: true, toggleShortcutReady: true, entries: [] };
        case 'model_download_links': f.calls.push(['links', args.kind, args.id]); return [{ url: 'https://huggingface.co/KBLab/kb-whisper-small/resolve/main/ggml-model-q5_0.bin', file: 'ggml-model-q5_0.bin' }];
        case 'plugin:opener|open_url': f.calls.push(['open', args.url]); return null;
        case 'plugin:dialog|open': return ['C:/Users/x/Downloads/ggml-model-q5_0.bin'];
        case 'import_model_files': f.calls.push(['import', args.kind, args.id, args.paths]);
          if (f.fail) throw 'ggml-model-q5_0.bin hör inte till den här modellen.';
          f.downloaded = true; return [];
        default: return null;
      }
    }, { shouldMockEvents: true });`);
  const step = async (name, fn) => { await fn(); console.log(`ok  ${name}`); };
  const dialog = () => page.getByRole('dialog', { name: 'Modeller på datorn' });
  try {
    await page.goto(process.env.AVSKRIFT_UI_URL || 'http://localhost:1420');
    await page.getByRole('heading', { name: 'Ditt arbete', exact: true }).waitFor();
    await page.getByRole('button', { name: 'Modeller på datorn', exact: true }).click();
    await dialog().waitFor();
    await step('a missing model offers the browser route', async () => {
      await dialog().getByRole('button', { name: 'Går det inte att hämta? Hämta i webbläsaren' }).first().click();
      await dialog().getByRole('button', { name: 'ggml-model-q5_0.bin' }).first().waitFor();
      assert.deepEqual((await page.evaluate(() => fixture.calls))[0], ['links', 'speech', 'kb-whisper-small']);
    });
    await step('the link opens in the browser', async () => {
      await dialog().getByRole('button', { name: 'ggml-model-q5_0.bin' }).first().click();
      await page.waitForFunction(() => fixture.calls.some(c => c[0] === 'open'));
      assert.equal((await page.evaluate(() => fixture.calls.find(c => c[0] === 'open')))[1], 'https://huggingface.co/KBLab/kb-whisper-small/resolve/main/ggml-model-q5_0.bin');
    });
    await step('a wrong file is explained', async () => {
      await page.evaluate(() => { fixture.fail = true; });
      await dialog().getByRole('button', { name: 'Välj hämtade filer…' }).first().click();
      await dialog().getByText('hör inte till den här modellen').first().waitFor();
      await page.evaluate(() => { fixture.fail = false; });
    });
    await step('picked files are put in place and the model counts as downloaded', async () => {
      await dialog().getByRole('button', { name: 'Välj hämtade filer…' }).first().click();
      await dialog().getByText('Finns på datorn').first().waitFor();
      const call = (await page.evaluate(() => fixture.calls)).filter(c => c[0] === 'import').at(-1);
      assert.deepEqual(call, ['import', 'speech', 'kb-whisper-small', ['C:/Users/x/Downloads/ggml-model-q5_0.bin']]);
      assert.equal(await dialog().getByRole('button', { name: 'Går det inte att hämta? Hämta i webbläsaren' }).count(), 0);
    });
    assert.deepEqual(errors, []);
    console.log('UI MODEL MANUAL: all steps passed');
  } catch (e) {
    fs.mkdirSync('.build-tools', { recursive: true });
    await page.screenshot({ path: '.build-tools/model-manual-failure.png' });
    console.error(e); console.error('page errors:', errors); process.exitCode = 1;
  } finally { await browser.close(); }
})();
