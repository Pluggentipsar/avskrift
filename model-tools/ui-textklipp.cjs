// Textklipp editor against `npm run dev` with mocked Tauri IPC.
// AVSKRIFT_TK_FIXTURE: project JSON (model-tools/ui-textklipp-fixture.cjs), AVSKRIFT_TK_PROXY: playable mp4.
// Both come from a local recording, so screenshots go to .build-tools/ (git-ignored), never docs/.
const { chromium } = require(process.env.AVSKRIFT_PLAYWRIGHT || 'playwright');
const fs = require('node:fs');
const assert = require('node:assert/strict');
const mocks = fs.readFileSync('node_modules/@tauri-apps/api/mocks.js', 'utf8').replace(/export \{[^}]+\};?/g, '');
const project = fs.readFileSync(process.env.AVSKRIFT_TK_FIXTURE, 'utf8');
const proxy = fs.readFileSync(process.env.AVSKRIFT_TK_PROXY);
const shots = '.build-tools/textklipp-ui';
fs.mkdirSync(shots, { recursive: true });

(async () => {
  const browser = await chromium.launch({ headless: true, channel: 'msedge' });
  const page = await browser.newPage({ viewport: { width: 1400, height: 900 } });
  const errors = []; page.on('pageerror', e => errors.push(e.message));
  // Serve the proxy for convertFileSrc URLs, with byte ranges so the video element can seek.
  await page.route('http://asset.localhost/**', route => {
    const range = /bytes=(\d+)-(\d*)/.exec(route.request().headers()['range'] || '');
    const start = range ? Number(range[1]) : 0, end = range && range[2] ? Number(range[2]) : proxy.length - 1;
    route.fulfill({ status: range ? 206 : 200, body: proxy.subarray(start, end + 1), headers: {
      'Content-Type': 'video/mp4', 'Accept-Ranges': 'bytes', 'Content-Length': String(end - start + 1),
      ...(range ? { 'Content-Range': `bytes ${start}-${end}/${proxy.length}` } : {}) } });
  });
  await page.addInitScript(mocks + `
    window.fixture = { project: ${project}, saves: [], previews: 0 };
    // Same rule as avskrift_textklipp::keep_ranges, cutting midway between items.
    function keepRanges(p, edits) {
      let id = 0; const items = [];
      for (const u of p.transcript.utterances) for (const w of u.words) items.push({ id: id++, start: w.start, end: w.end });
      for (const s of p.sounds) items.push({ id: s.id, start: s.start, end: s.end });
      items.sort((a, b) => a.start - b.start);
      const del = new Set(edits.deleted), removed = [];
      for (let i = 0; i < items.length; i++) {
        if (!del.has(items[i].id)) continue;
        const first = i; while (i + 1 < items.length && del.has(items[i + 1].id)) i++;
        removed.push([first ? (items[first - 1].end + items[first].start) / 2 : 0, i + 1 < items.length ? (items[i].end + items[i + 1].start) / 2 : p.media.duration]);
      }
      if (edits.pauseLimit != null) for (const [a, b] of p.pauses) if (b - a > edits.pauseLimit) removed.push([a + edits.pauseLimit / 2, b - edits.pauseLimit / 2]);
      removed.sort((a, b) => a[0] - b[0]);
      const keep = []; let t = 0;
      for (const [a, b] of removed) { if (a - t >= 0.04) keep.push([t, a]); t = Math.max(t, b); }
      if (p.media.duration - t >= 0.04) keep.push([t, p.media.duration]);
      return keep;
    }
    mockWindows('main');
    mockConvertFileSrc('windows');
    mockIPC(async (cmd, args) => {
      const f = window.fixture;
      switch (cmd) {
        case 'begin_work': return 'w-' + Math.random();
        case 'forget_work': return; case 'cancel_work': return true;
        case 'plugin:app|version': return '0.7.0-beta.2';
        case 'list_whisper_models': return [{ id: 'kb-whisper-large', label: 'KB-Whisper large', sizeMb: 1030, downloaded: true }];
        case 'list_summary_models': return [{ id: 'qwen2.5-3b', label: 'Qwen', sizeMb: 2000, downloaded: true }];
        case 'list_summary_templates': return [];
        case 'list_jobs': case 'search_jobs': case 'list_all_actions': case 'list_actions': case 'list_standalone_tasks': return [];
        case 'dictation_snapshot': return { revision: 1, phase: 'idle', message: '', supported: true, backend: 'CPU', settings: { model: 'kb-whisper-large', saveHistory: false, enabled: true, autoInsert: true }, holdShortcutReady: true, toggleShortcutReady: true, entries: [] };
        case 'wordalign_status': return { ready: true, available: true, sizeMb: 632 };
        case 'textklipp_list': return [{ id: f.project.id, title: f.project.title, updatedAt: f.project.updatedAt, duration: f.project.media.duration, status: 'ready', hasProxy: true }];
        case 'textklipp_open': return structuredClone(f.project);
        case 'textklipp_media': return { playback: 'C:/synthetic/textklipp/tk-fixture/proxy.mp4', isVideo: true };
        case 'textklipp_preview': { f.previews++; const keep = keepRanges(f.project, args.edits); return { keep, editedDuration: keep.reduce((s, [a, b]) => s + b - a, 0) }; }
        case 'textklipp_save_edits': f.saves.push(structuredClone(args.edits)); f.project.edits = structuredClone(args.edits); return '2026-10-02T10:01:00Z';
        default: return null;
      }
    }, { shouldMockEvents: true });
  `);

  const step = async (name, fn) => { const t = Date.now(); await fn(); console.log(`ok  ${name} (${Date.now() - t} ms)`); };
  // Select the visible text of word spans [from..to] (inclusive), as a mouse drag would.
  const selectWords = (from, to) => page.evaluate(([a, b]) => {
    const s = document.querySelector(`[data-id="${a}"]`), e = document.querySelector(`[data-id="${b}"]`);
    const r = document.createRange(); r.setStart(s.firstChild, 0); r.setEnd(e.firstChild, e.firstChild.length);
    const sel = getSelection(); sel.removeAllRanges(); sel.addRange(r);
  }, [from, to]);
  const idOf = text => page.evaluate(t => [...document.querySelectorAll('.doc [data-id]')].find(e => e.textContent === t)?.dataset.id, text).then(Number);
  const lastSave = () => page.evaluate(() => window.fixture.saves.at(-1));

  try {
    await page.goto(process.env.AVSKRIFT_UI_URL || 'http://localhost:1420');
    await step('home has a Textklipp entry point', async () => {
      await page.getByRole('heading', { name: 'Ditt arbete', exact: true }).waitFor();
      await page.getByRole('button', { name: /Textklipp/ }).first().waitFor();
    });
    await step('navigation opens the project list', async () => {
      await page.getByRole('navigation', { name: 'Huvudnavigation' }).getByRole('button', { name: 'Textklipp' }).click();
      await page.getByRole('heading', { name: 'Textklipp', exact: true }).waitFor();
      await page.getByRole('button', { name: /Testklipp/ }).waitFor();
      await page.screenshot({ path: `${shots}/list.png` });
    });
    await step('opening a project shows video, timeline and 425 words + sounds', async () => {
      await page.getByRole('button', { name: /Testklipp/ }).click();
      await page.locator('.doc [data-id]').first().waitFor();
      assert.equal(await page.locator('.doc span[data-id]:not(.sound)').count(), 425);
      assert.equal(await page.locator('.doc span.sound').count(), 10);
      await page.waitForFunction(() => document.querySelector('video')?.readyState >= 1);
    });
    let egentligen;
    await step('select a word + Delete strikes it, saves and cuts', async () => {
      egentligen = await idOf('egentligen');
      await selectWords(egentligen, egentligen);
      await page.keyboard.press('Delete');
      await page.locator(`[data-id="${egentligen}"].struck`).waitFor();
      await page.waitForFunction(() => window.fixture.saves.length > 0);
      assert.deepEqual((await lastSave()).deleted, [egentligen]);
      await page.locator('.stats dd').nth(2).filter({ hasText: '1' }).waitFor();
    });
    await step('Ctrl+Z restores, Ctrl+Y strikes again', async () => {
      await page.keyboard.press('Control+z');
      await page.waitForFunction(id => !document.querySelector(`[data-id="${id}"]`).classList.contains('struck'), egentligen);
      await page.keyboard.press('Control+y');
      await page.locator(`[data-id="${egentligen}"].struck`).waitFor();
    });
    let introEnd;
    await step('a dragged range across the intro is struck as one cut', async () => {
      introEnd = await idOf('enkelt.');
      await selectWords(0, introEnd);
      await page.keyboard.press('Delete');
      // Every word and sound block shown between the first word and "enkelt." is struck.
      const expected = await page.evaluate(end => {
        const all = [...document.querySelectorAll('.doc [data-id]')].map(e => Number(e.dataset.id));
        return all.slice(0, all.indexOf(end) + 1);
      }, introEnd);
      assert.ok(expected.some(id => id >= 1000000), 'the intro contains sound blocks');
      await page.waitForFunction(ids => { const d = window.fixture.saves.at(-1)?.deleted ?? []; return ids.every(id => d.includes(id)); }, expected);
      await page.locator('.stats dd').nth(2).filter({ hasText: '2' }).waitFor();
      await page.screenshot({ path: `${shots}/editor-struck.png` });
    });
    await step('selecting struck text + Delete restores it', async () => {
      await selectWords(egentligen, egentligen);
      await page.keyboard.press('Delete');
      await page.waitForFunction(id => !document.querySelector(`[data-id="${id}"]`).classList.contains('struck'), egentligen);
      await selectWords(egentligen, egentligen); await page.keyboard.press('Delete'); // strike again for playback test
      await page.locator(`[data-id="${egentligen}"].struck`).waitFor();
    });
    await step('playback starts after the cut intro and skips the struck word', async () => {
      await page.getByRole('button', { name: 'Spela' }).click();
      await page.waitForFunction(() => !document.querySelector('video').paused);
      const start = await page.evaluate(() => document.querySelector('video').currentTime);
      // It begins between the last struck intro item and the first item still kept.
      const [lastStruckEnd, firstKeptStart] = await page.evaluate(() => {
        const p = window.fixture.project, del = new Set(p.edits.deleted); let id = 0;
        const items = p.transcript.utterances.flatMap(u => u.words.map(w => ({ id: id++, ...w }))).concat(p.sounds).sort((a, b) => a.start - b.start);
        const first = items.findIndex(i => !del.has(i.id));
        return [items[first - 1].end, items[first].start];
      });
      assert.ok(start >= lastStruckEnd - 0.01 && start <= firstKeptStart + 0.01,
        `playback should begin between ${lastStruckEnd} and ${firstKeptStart}, began at ${start}`);
      // Jump just before "egentligen" and play through it.
      const at = await page.evaluate(id => window.fixture.project.transcript.utterances.flatMap(u => u.words)[id].start, egentligen);
      await page.evaluate(t => { const v = document.querySelector('video'); v.currentTime = t - 1.2; }, at);
      const seen = await page.evaluate(([s, e]) => new Promise(done => {
        const v = document.querySelector('video'), times = []; const until = performance.now() + 2500;
        (function tick() { times.push(v.currentTime); if (performance.now() < until) requestAnimationFrame(tick); else done(times); })();
      }), [at, at]);
      const w = await page.evaluate(id => window.fixture.project.transcript.utterances.flatMap(u => u.words)[id], egentligen);
      assert.ok(!seen.some(t => t > w.start + 0.05 && t < w.end - 0.05), 'a struck word must never be played');
      assert.ok(seen.at(-1) > w.end, 'playback continues after the cut');
      await page.getByRole('button', { name: 'Pausa' }).click();
    });
    await step('current word is highlighted while playing', async () => {
      assert.equal(await page.locator('.doc .current').count(), 1);
    });
    await step('hiding struck text and shortening pauses', async () => {
      await page.getByLabel('Visa borttagen text').uncheck();
      assert.equal(await page.locator('.doc .struck').count(), 0);
      const before = await page.evaluate(() => window.fixture.previews);
      await page.getByLabel('Korta pauser').selectOption('1');
      await page.waitForFunction(() => window.fixture.saves.at(-1)?.pauseLimit === 1);
      assert.ok(await page.evaluate(b => window.fixture.previews > b, before));
      await page.screenshot({ path: `${shots}/editor-hidden.png` });
      await page.getByLabel('Visa borttagen text').check();
    });
    await step('strike all sound blocks, then restore everything', async () => {
      await page.getByRole('button', { name: 'Ta bort alla' }).click();
      await page.waitForFunction(() => window.fixture.saves.at(-1)?.deleted.filter(id => id >= 1000000).length === 10);
      await page.getByRole('button', { name: 'Återställ allt' }).click();
      await page.waitForFunction(() => { const s = window.fixture.saves.at(-1); return s.deleted.length === 0 && s.pauseLimit === null; });
      await page.locator('.stats dd').nth(2).filter({ hasText: /^0$/ }).waitFor();
    });
    await step('back to the list saves and closes', async () => {
      await page.getByRole('button', { name: '← Alla klipp' }).click();
      await page.getByRole('button', { name: 'Välj video eller ljudfil…' }).waitFor();
    });
    assert.deepEqual(errors, []);
    console.log('UI TEXTKLIPP: all steps passed');
  } catch (e) {
    await page.screenshot({ path: `${shots}/failure.png`, fullPage: true }).catch(() => {});
    console.error(e); console.error('page errors:', errors); process.exitCode = 1;
  } finally { await browser.close(); }
})();
