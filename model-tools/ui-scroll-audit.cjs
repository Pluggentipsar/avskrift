// Can the user scroll to the end of long content? Mouse-wheel only (no programmatic scrolling,
// which also works on overflow:hidden and would hide the bug). Run against `npm run dev`.
const { chromium } = require(process.env.AVSKRIFT_PLAYWRIGHT || 'playwright');
const fs = require('node:fs');
const mocks = fs.readFileSync('node_modules/@tauri-apps/api/mocks.js', 'utf8').replace(/export \{[^}]+\};?/g, '');
const long = Array.from({ length: 120 }, (_, i) => `Stycke ${i + 1}. Anna Andersson ringde från Jönköping om beställningen och vi gick igenom detaljerna noggrant.`).join('\n\n') + '\n\nSLUTET AV TEXTEN';

(async () => {
  const browser = await chromium.launch({ headless: true, channel: 'msedge' });
  const results = [];
  for (const [w, h] of [[1280, 720], [1440, 900]]) {
    const page = await browser.newPage({ viewport: { width: w, height: h } });
    await page.addInitScript(mocks + `
      const LONG = ${JSON.stringify(long)};
      const words = LONG.split(/(\\s+)/).filter(Boolean);
      let pos = 0; const segments = words.map(t => { const s = { text: t, span: null, start: pos, end: pos + t.length, word: /\\S/.test(t), para: 0 }; pos += t.length; return s; });
      const transcript = { language: 'sv', model: 'kb-whisper-small', diarized: false, utterances: [{ start: 0, end: 5, speaker: null, text: LONG, words: [] }] };
      const job = { version: 1, id: 'j1', jobType: 'transcribe', title: 'Lång text', createdAt: '2026-10-02T08:00:00Z', updatedAt: '2026-10-02T08:00:00Z', transcript, summaryDraft: LONG, category: '', actions: [], participants: [], enabled: ['person'], rejected: [] };
      mockWindows('main'); mockConvertFileSrc('windows');
      mockIPC(async (cmd, args) => {
        switch (cmd) {
          case 'begin_work': return 'w'; case 'plugin:app|version': return 'audit';
          case 'list_whisper_models': return [{ id: 'kb-whisper-small', label: 'Small', sizeMb: 1, downloaded: true }];
          case 'list_summary_models': return [{ id: 'qwen2.5-3b', label: 'Qwen', sizeMb: 1, downloaded: true }];
          case 'list_summary_templates': return [{ id: 'protokoll', label: 'Protokoll' }];
          case 'list_jobs': case 'search_jobs': return [job];
          case 'open_job': return structuredClone(job);
          case 'list_all_actions': case 'list_actions': case 'list_standalone_tasks': return [];
          case 'dictation_snapshot': return { revision: 1, phase: 'idle', message: '', supported: true, backend: 'CPU', settings: { model: 'kb-whisper-small', saveHistory: false, enabled: true, autoInsert: true }, holdShortcutReady: true, toggleShortcutReady: true, entries: [] };
          case 'analyze_document': return { text: args.args.text, segments, spans: [], counts: {}, warnings: [] };
          case 'summarize_source': case 'summarize': return LONG;
          default: return null;
        }
      }, { shouldMockEvents: true });`);
    await page.goto(process.env.AVSKRIFT_UI_URL || 'http://localhost:1420');
    await page.getByRole('heading', { name: 'Ditt arbete', exact: true }).waitFor();

    // Wheel over a point until nothing moves, then report whether `locator` is fully on screen.
    async function reachable(label, center, target) {
      const box = await center.boundingBox();
      for (let i = 0; i < 80; i++) await page.mouse.wheel(0, 600), page.mouse.move(box.x + box.width / 2, box.y + Math.min(box.height / 2, 200));
      await page.waitForTimeout(200);
      const b = await target.boundingBox();
      const ok = !!b && b.y + b.height <= h + 1 && b.y >= -1;
      // Also: is the target clipped by an ancestor with overflow hidden?
      const clipped = await target.evaluate(el => {
        const r = el.getBoundingClientRect();
        for (let a = el.parentElement; a; a = a.parentElement) {
          const s = getComputedStyle(a), ar = a.getBoundingClientRect();
          if (s.overflowY !== 'visible' && (r.bottom > ar.bottom + 1)) return `${a.tagName.toLowerCase()}.${[...a.classList].join('.')} (${s.overflowY})`;
        }
        return null;
      });
      results.push(`${w}x${h} ${label}: ${ok && !clipped ? 'OK' : 'CANNOT REACH END' + (clipped ? ' – clipped by ' + clipped : '')}`);
      await page.screenshot({ path: `.build-tools/scroll-audit/${w}x${h}-${label}.png` });
    }
    fs.mkdirSync('.build-tools/scroll-audit', { recursive: true });

    // Avidentifiering: paste the long text and analyse.
    await page.getByRole('navigation', { name: 'Huvudnavigation' }).getByRole('button', { name: 'Avidentifiering' }).click();
    await page.screenshot({ path: '.build-tools/scroll-audit/deid-start.png' });
    console.log('deid controls:', await page.evaluate(() => [...document.querySelectorAll('textarea,input[type=radio],button')].slice(0, 40).map(e => (e.getAttribute('aria-label') || e.value || e.textContent || '').trim().slice(0, 40)).join(' | ')));
    await page.locator('textarea.src-text:visible').fill(long);
    await page.getByRole('button', { name: /^Avidentifiera/ }).first().click();
    await page.locator('.document').waitFor();
    await reachable('avidentifiering-text', page.locator('.document'), page.locator('main.review .document .maskword').last());
    await reachable('avidentifiering-slut', page.locator('.document'), page.locator('.reassure'));

    // Summary of a transcript (job view, Sammanfattning tab).
    await page.getByRole('navigation', { name: 'Huvudnavigation' }).getByRole('button', { name: 'Ditt arbete' }).click();
    await page.getByRole('button', { name: /Lång text/ }).first().click();
    await page.getByRole('button', { name: 'Sammanfattning', exact: true }).click();
    const ta = page.getByLabel('Sammanfattning – redigerbart utkast');
    await ta.waitFor();
    // A textarea scrolls internally; check the box itself fits on screen.
    // Wheel over the page beside the box (over the box the wheel scrolls the box's own text).
    await reachable('sammanfattning-ruta', page.locator('main.review .banner.warn').last(), ta);
    // Summarize-text screen.
    await page.getByRole('button', { name: 'Sammanfatta text' }).click();
    await page.locator('input[type=radio][value=paste]:visible').check();
    await page.locator('textarea.src-text:visible').fill(long);
    await page.getByRole('button', { name: /Skapa sammanfattning|Generera om/ }).click();
    const ta2 = page.locator('main.review .summary-edit');
    await ta2.waitFor();
    await reachable('sammanfatta-text-ruta', ta2, ta2);
    await page.close();
  }
  console.log(results.join('\n'));
  await browser.close();
})();
