// Summary as markdown: formatted text pasted from an AI chat keeps its structure, can be viewed
// formatted (tables as tables), copied with formatting and exported to Word as markdown.
// Mocked IPC with synthetic data.
const { chromium } = require(process.env.AVSKRIFT_PLAYWRIGHT || 'playwright');
const fs = require('node:fs');
const assert = require('node:assert/strict');
const mocks = fs.readFileSync('node_modules/@tauri-apps/api/mocks.js', 'utf8').replace(/export \{[^}]+\};?/g, '');
const CHAT_HTML = `<h2>Sammanfattning</h2><p>Mötet handlade om <strong>budgeten</strong>.</p><ul><li>Anna tar underlag</li><li>Karim bokar lokal</li></ul>
<table><thead><tr><th>Vem</th><th>Vad</th></tr></thead><tbody><tr><td>Anna</td><td>Underlag</td></tr><tr><td>Karim</td><td>Lokal | rum 2</td></tr></tbody></table>
<script>window.ran = true</script><img src=x onerror="window.ran=true">`;

(async () => {
  const browser = await chromium.launch({ headless: true, channel: 'msedge' });
  const url = process.env.AVSKRIFT_UI_URL || 'http://localhost:1420';
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, permissions: ['clipboard-read', 'clipboard-write'] });
  const page = await context.newPage();
  const errors = []; page.on('pageerror', e => errors.push(e.message));
  await page.addInitScript(mocks + `
    const utterances = [{ start: 0, end: 4, speaker: null, text: 'Vi pratade om budgeten.', words: [] }];
    const job = { version: 1, id: 'm1', jobType: 'meeting', title: 'Veckomöte', createdAt: '2026-10-02T08:00:00Z', updatedAt: '2026-10-02T08:00:00Z',
      transcript: { language: 'sv', model: 'kb-whisper-small', diarized: false, utterances }, lastView: 'summary', summaryDraft: '',
      actions: [], category: '', participants: [], enabled: [], rejected: [] };
    window.fixture = { jobs: [job], exports: [] };
    mockWindows('main'); mockConvertFileSrc('windows');
    mockIPC(async (cmd, args) => {
      const f = window.fixture;
      switch (cmd) {
        case 'begin_work': return 'w'; case 'plugin:app|version': return 'test';
        case 'list_whisper_models': return [{ id: 'kb-whisper-small', label: 'Small', sizeMb: 1, downloaded: true }];
        case 'list_summary_models': return [{ id: 'qwen2.5-3b', label: 'Qwen', sizeMb: 1, downloaded: true }];
        case 'list_jobs': case 'search_jobs': return structuredClone(f.jobs);
        case 'open_job': return structuredClone(f.jobs.find(j => j.id === args.id));
        case 'save_job': { const j = structuredClone(args.job ?? args); const i = f.jobs.findIndex(x => x.id === j.id); if (i >= 0) f.jobs[i] = { ...f.jobs[i], ...j }; return; }
        case 'list_all_actions': case 'list_actions': case 'list_standalone_tasks': case 'list_summary_templates': return [];
        case 'plugin:dialog|save': return 'C:/tmp/utkast.docx';
        case 'save_summary': f.exports.push(args.args); return;
        case 'dictation_snapshot': return { revision: 1, phase: 'idle', message: '', supported: true, backend: 'CPU', settings: { model: 'kb-whisper-small', saveHistory: false, enabled: true, autoInsert: true }, holdShortcutReady: true, toggleShortcutReady: true, entries: [] };
        default: return null;
      }
    }, { shouldMockEvents: true });`);
  const step = async (name, fn) => { await fn(); console.log(`ok  ${name}`); };
  const setClipboard = (html, text) => page.evaluate(([html, text]) => navigator.clipboard.write([new ClipboardItem({
    'text/html': new Blob([html], { type: 'text/html' }), 'text/plain': new Blob([text], { type: 'text/plain' }) })]), [html, text]);
  const draft = () => page.evaluate(() => fixture.jobs[0].summaryDraft);
  try {
    await page.goto(url);
    await page.getByRole('heading', { name: 'Ditt arbete', exact: true }).waitFor();
    await page.getByRole('button', { name: /Veckomöte/ }).first().click();
    await page.getByRole('button', { name: 'Sammanfattning', exact: true }).click();
    await step('an empty summary offers pasting from an external AI, keeping tables and headings', async () => {
      await setClipboard(CHAT_HTML, 'Sammanfattning Mötet handlade om budgeten.');
      await page.getByRole('button', { name: 'Klistra in från AI', exact: true }).click();
      await page.waitForFunction(() => (fixture.jobs[0].summaryDraft || '').includes('| Vem | Vad |'));
      const md = await draft();
      assert.match(md, /^## Sammanfattning/m);
      assert.match(md, /\*\*budgeten\*\*/);
      assert.match(md, /^- Anna tar underlag$/m);
      assert.match(md, /\| Karim \| Lokal \\\| rum 2 \|/, 'pipes inside cells are escaped');
      assert.ok(!/script|onerror/.test(md), 'scripts are not carried over');
    });
    await step('the formatted view shows a real table and never runs pasted markup', async () => {
      const view = page.getByLabel('Sammanfattning – formaterad');
      await view.waitFor();
      assert.equal(await view.locator('table tr').count(), 3);
      assert.equal(await view.locator('h2').innerText(), 'Sammanfattning');
      assert.equal(await page.evaluate(() => window.ran === true), false);
    });
    await step('pasting formatted text into the editor converts it at the cursor', async () => {
      await page.getByRole('button', { name: 'Redigera', exact: true }).click();
      const box = page.getByLabel('Sammanfattning – redigerbart utkast');
      await box.evaluate(el => { el.focus(); el.selectionStart = el.selectionEnd = el.value.length; });
      await box.evaluate(el => {
        const dt = new DataTransfer();
        dt.setData('text/html', '<h3>Nästa steg</h3><ol><li>Skicka protokoll</li></ol>');
        dt.setData('text/plain', 'Nästa steg Skicka protokoll');
        el.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true }));
      });
      await page.waitForFunction(() => (fixture.jobs[0].summaryDraft || '').includes('### Nästa steg'));
      assert.match(await draft(), /^1\. Skicka protokoll$/m);
    });
    await step('pasting from AI onto an existing draft asks replace or append', async () => {
      await setClipboard('<p>Kort <em>tillägg</em>.</p><table><tr><td>a</td><td>b</td></tr></table>', 'Kort tillägg.');
      await page.locator('.summary-tools').getByRole('button', { name: 'Klistra in från AI' }).click();
      await page.getByRole('button', { name: 'Lägg till sist', exact: true }).click();
      await page.waitForFunction(() => (fixture.jobs[0].summaryDraft || '').includes('tillägg'));
      const md = await draft();
      assert.match(md, /### Nästa steg[\s\S]*Kort \*tillägg\*\./, 'appended after the existing text');
    });
    await step('copy puts formatting and markdown on the clipboard', async () => {
      await page.getByRole('button', { name: 'Kopiera', exact: true }).click();
      await page.getByText('Kopierat till urklipp').waitFor();
      const [html, text] = await page.evaluate(async () => {
        const [item] = await navigator.clipboard.read();
        const [h, t] = await Promise.all([item.getType('text/html'), item.getType('text/plain')]);
        return [await h.text(), await t.text()];
      });
      assert.match(html, /<table>/);
      assert.match(text, /\| Vem \| Vad \|/);
    });
    await step('Word export is told the text is markdown', async () => {
      await page.getByRole('button', { name: 'Exportera…', exact: true }).click();
      const dialog = page.getByRole('dialog', { name: 'Granska och exportera' });
      await dialog.waitFor();
      await dialog.getByLabel('Format', { exact: true }).selectOption('docx');
      await dialog.getByRole('button', { name: 'Spara fil…' }).click();
      await page.waitForFunction(() => fixture.exports.length > 0);
      const saved = await page.evaluate(() => fixture.exports.at(-1));
      assert.equal(saved.markdown, true);
      assert.match(saved.text, /\| Vem \| Vad \|/);
    });
    assert.deepEqual(errors, []);
    console.log('UI SUMMARY MARKDOWN: all steps passed');
  } catch (e) {
    fs.mkdirSync('.build-tools', { recursive: true });
    await page.screenshot({ path: '.build-tools/summary-markdown-failure.png', fullPage: true });
    console.error(e); console.error('page errors:', errors); process.exitCode = 1;
  } finally { await browser.close(); }
})();
