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
      removed.push(...(edits.removed ?? []));
      removed.sort((a, b) => a[0] - b[0]);
      let keep = []; let t = 0;
      for (const [a, b] of removed) { if (a - t >= 0.04) keep.push([t, a]); t = Math.max(t, b); }
      if (p.media.duration - t >= 0.04) keep.push([t, p.media.duration]);
      keep.push(...(edits.kept ?? [])); keep.sort((a, b) => a[0] - b[0]);
      const merged = [];
      for (const [a, b] of keep) { const l = merged.at(-1); if (l && a <= l[1] + 1e-9) l[1] = Math.max(l[1], b); else merged.push([a, b]); }
      return merged.filter(([a, b]) => b - a >= 0.04);
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
        case 'textklipp_preview': { f.previews++; const keep = keepRanges(f.project, args.edits); window.__lastPreview = { keep }; return { keep, editedDuration: keep.reduce((s, [a, b]) => s + b - a, 0) }; }
        case 'textklipp_silences': return (f.silences ?? []).filter(([a, b]) => b - a >= args.min);
        case 'textklipp_waveform': return Array.from({ length: args.bars }, (_, i) => 0.5 + 0.4 * Math.sin((args.start + (args.end - args.start) * i / args.bars) * 9));
        case 'plugin:dialog|save': f.saveDialog = args; return 'C:/synthetic/Testklipp (klippt).mp4';
        case 'plugin:opener|reveal_item_in_dir': f.revealed = args; return;
        case 'textklipp_export': f.exported = structuredClone(args); await new Promise(r => setTimeout(r, 300));
          return { output: args.args.output, extra: args.args.srt ? ['C:/synthetic/Testklipp (klippt).srt'] : [], expectedDuration: 154, videoDuration: 153.999, audioDuration: 153.999, syncOk: true, encoder: 'h264_nvenc', pieces: 50, seconds: 19.6 };
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
    await step('detail view shows waveform around the playhead', async () => {
      await page.locator('canvas[data-view]').waitFor();
      assert.ok(await page.evaluate(() => document.querySelector('canvas[data-view]').width > 100));
    });
    await step('search finds all three takes and Enter jumps between them', async () => {
      const box = page.getByRole('searchbox', { name: 'Sök i texten' });
      await box.fill('heter joel');
      await page.getByText('1 av 3').waitFor();
      await box.press('Enter');
      const first = await page.evaluate(() => document.querySelector('video').currentTime);
      await box.press('Enter');
      await page.getByText('2 av 3').waitFor();
      const second = await page.evaluate(() => document.querySelector('video').currentTime);
      assert.ok(first > 30 && second > first + 20, `${first} → ${second}`);
      assert.ok(await page.locator('.doc .hit').count() >= 6);
      await box.fill('');
    });
    await step('go to time', async () => {
      const t = page.getByLabel('Gå till tid (minuter:sekunder)');
      await t.fill('1:30'); await t.press('Enter');
      await page.waitForFunction(() => Math.abs(document.querySelector('video').currentTime - 90) < 0.5);
      await t.fill('99:00'); await t.press('Enter');
      assert.ok(await t.evaluate(e => e.classList.contains('invalid')));
      await t.fill('');
    });
    await step('retake suggestions: remove the first take, keep the last', async () => {
      await page.getByText('2 möjliga omtagningar').waitFor();
      await page.getByRole('button', { name: 'Ta bort tidigare tagning' }).first().click();
      await page.waitForFunction(() => window.fixture.saves.at(-1)?.deleted.length > 50);
      assert.equal(await page.locator('.retakes li.done').count(), 1);
      await page.keyboard.press('Control+z');
      await page.waitForFunction(() => window.fixture.saves.at(-1)?.deleted.length < 50);
    });
    await step('nudging the active cut by one frame stores a manual range', async () => {
      const at = await page.evaluate(id => window.fixture.project.transcript.utterances.flatMap(u => u.words)[id].start, egentligen);
      await page.evaluate(t => { document.querySelector('video').currentTime = t - 0.6; }, at);
      // The label shows the cut's real times (a broken template once rendered "Klipp ,–,").
      await page.waitForFunction(() => /^Klipp \d+:\d\d,\d–\d+:\d\d,\d \(/.test(document.querySelector('.nudge span')?.textContent ?? ''));
      await page.getByRole('button', { name: 'Börja klippet en bildruta tidigare' }).click();
      await page.waitForFunction(() => (window.fixture.saves.at(-1)?.removed ?? []).length === 1);
      const [a, b] = await page.evaluate(() => window.fixture.saves.at(-1).removed[0]);
      assert.ok(Math.abs(b - a - 1 / 30) < 0.002, `one frame at 30 fps, got ${b - a}`);
      await page.getByRole('button', { name: 'Sluta klippet en bildruta tidigare' }).click();
      await page.waitForFunction(() => (window.fixture.saves.at(-1)?.kept ?? []).length === 1);
      await page.keyboard.press('Control+z'); await page.keyboard.press('Control+z');
      await page.waitForFunction(() => { const s = window.fixture.saves.at(-1); return !s.removed.length && !s.kept.length; });
    });
    await step('dragging a cut edge in the detail view moves the cut', async () => {
      const canvas = page.locator('canvas[data-view]');
      await canvas.evaluate(c => c.scrollIntoView({ block: 'center' }));
      const box = await canvas.boundingBox();
      const { view, span } = await page.evaluate(() => {
        const c = document.querySelector('canvas[data-view]');
        return { view: Number(c.dataset.view), span: Number(c.dataset.span) };
      });
      // A cut starts where a kept range ends; take the first one inside the detail view.
      const preview = await page.evaluate(() => window.__lastPreview);
      const g = preview.keep.map(k => k[1]).find(b => b > view && b < view + span);
      assert.ok(g, 'a cut start is visible in the detail view');
      const x = box.x + ((g - view) / span) * box.width, y = box.y + 60;
      await page.mouse.move(x, y); await page.mouse.down();
      await page.mouse.move(x - box.width * 0.04, y, { steps: 4 }); await page.mouse.up();
      await page.waitForFunction(() => (window.fixture.saves.at(-1)?.removed ?? []).length === 1);
      const [a, b] = await page.evaluate(() => window.fixture.saves.at(-1).removed[0]);
      assert.ok(Math.abs(b - g) < 0.04 && b - a > 0.2, `dragged ${a}-${b} from edge ${g}`);
      await page.keyboard.press('Control+z');
      await page.waitForFunction(() => !(window.fixture.saves.at(-1)?.removed ?? []).length);
    });
    await step('marking in and out at the playhead and Delete removes that time', async () => {
      const goto = async t => {
        await page.getByLabel('Gå till tid (minuter:sekunder)').fill(t);
        await page.getByLabel('Gå till tid (minuter:sekunder)').press('Enter');
        await page.evaluate(() => { document.activeElement?.blur(); window.getSelection().removeAllRanges(); });
      };
      await page.evaluate(() => document.querySelector('video').pause());
      await goto('40');
      await page.keyboard.press('i');
      await goto('43,5');
      await page.keyboard.press('o');
      await page.locator('.markinfo').filter({ hasText: /\(3,5 s\)/ }).waitFor();
      await page.keyboard.press('Delete');
      await page.waitForFunction(() => (window.fixture.saves.at(-1)?.removed ?? []).length === 1);
      const [a, b] = await page.evaluate(() => window.fixture.saves.at(-1).removed[0]);
      assert.ok(Math.abs(a - 40) < 0.05 && Math.abs(b - 43.5) < 0.05, `removed ${a}-${b}`);
      assert.ok(await page.locator('.doc .cutaway').count() > 0, 'words in the removed time are shown as cut');
      assert.equal(await page.locator('.markinfo').count(), 0, 'the mark is cleared after use');
      await page.keyboard.press('Control+z');
      await page.waitForFunction(() => !(window.fixture.saves.at(-1)?.removed ?? []).length);
    });
    await step('dragging over the waveform marks a range to remove or keep', async () => {
      const canvas = page.locator('canvas[data-view]');
      await canvas.evaluate(c => c.scrollIntoView({ block: 'center' }));
      const box = await canvas.boundingBox();
      const { view, span } = await page.evaluate(() => { const c = document.querySelector('canvas[data-view]'); return { view: Number(c.dataset.view), span: Number(c.dataset.span) }; });
      const edges = (await page.evaluate(() => window.__lastPreview.keep)).flat();
      // A stretch of about 1.6 s without cut edges near its ends.
      let from = view + 0.5;
      while (edges.some(e => Math.abs(e - from) < 0.4 || Math.abs(e - (from + 1.6)) < 0.4) && from < view + span - 2.5) from += 0.2;
      const px = t => box.x + ((t - view) / span) * box.width, y = box.y + 50;
      await page.mouse.move(px(from), y); await page.mouse.down();
      await page.mouse.move(px(from + 1.6), y, { steps: 6 }); await page.mouse.up();
      await page.getByRole('button', { name: /Ta bort markerat/ }).click();
      await page.waitForFunction(() => (window.fixture.saves.at(-1)?.removed ?? []).length === 1);
      const [a, b] = await page.evaluate(() => window.fixture.saves.at(-1).removed[0]);
      assert.ok(Math.abs(b - a - 1.6) < 0.1, `marked ${a}-${b}`);
      await page.keyboard.press('Control+z');
      await page.waitForFunction(() => !(window.fixture.saves.at(-1)?.removed ?? []).length);
    });
    await step('scissors: split twice, select the piece between and remove it, as in a video editor', async () => {
      const canvas = page.locator('canvas[data-view]');
      await canvas.evaluate(c => c.scrollIntoView({ block: 'center' }));
      const box = await canvas.boundingBox();
      const { view, span } = await page.evaluate(() => { const c = document.querySelector('canvas[data-view]'); return { view: Number(c.dataset.view), span: Number(c.dataset.span) }; });
      const edges = (await page.evaluate(() => window.__lastPreview.keep)).flat();
      let from = view + 0.6;
      while (edges.some(e => e > from - 0.3 && e < from + 2.3) && from < view + span - 3) from += 0.2;
      const px = t => box.x + ((t - view) / span) * box.width, y = box.y + 50;
      await page.getByRole('button', { name: 'Sax', exact: true }).click();
      assert.equal(await page.getByRole('button', { name: 'Sax', exact: true }).getAttribute('aria-pressed'), 'true');
      await page.mouse.click(px(from), y);
      await page.mouse.click(px(from + 2), y);
      await page.waitForFunction(() => (window.fixture.saves.at(-1)?.splits ?? []).length === 2);
      assert.equal(await page.locator('.timeline .split').count(), 2);
      // Clicking a split again with the scissors removes it; S splits at the playhead.
      await page.mouse.click(px(from + 2), y);
      await page.waitForFunction(() => (window.fixture.saves.at(-1)?.splits ?? []).length === 1);
      await page.keyboard.press('Control+z');
      await page.waitForFunction(() => (window.fixture.saves.at(-1)?.splits ?? []).length === 2);
      await page.keyboard.press('v');
      assert.equal(await page.getByRole('button', { name: 'Markera', exact: true }).getAttribute('aria-pressed'), 'true');
      await page.mouse.click(px(from + 1), y);
      await page.locator('.markinfo').filter({ hasText: /\(2 s\)/ }).waitFor();
      await page.evaluate(() => { document.activeElement?.blur(); window.getSelection().removeAllRanges(); });
      await page.keyboard.press('Delete');
      await page.waitForFunction(() => (window.fixture.saves.at(-1)?.removed ?? []).length === 1);
      const [a, b] = await page.evaluate(() => window.fixture.saves.at(-1).removed[0]);
      assert.ok(Math.abs(b - a - 2) < 0.05, `removed the piece ${a}-${b}`);
      const n = await page.evaluate(() => window.fixture.saves.at(-1).splits.length);
      await page.keyboard.press('s');
      await page.waitForFunction(n => (window.fixture.saves.at(-1)?.splits ?? []).length === n + 1, n);
      await page.keyboard.press('Control+z'); await page.keyboard.press('Control+z'); await page.keyboard.press('Control+z'); await page.keyboard.press('Control+z');
      await page.waitForFunction(() => { const s = window.fixture.saves.at(-1); return !s.removed.length && !(s.splits ?? []).length; });
    });
    await step('long silences are marked in the text and removed with a margin, one or all', async () => {
      await page.evaluate(() => { window.fixture.silences = [[50, 54], [90, 92.6]]; });
      await page.locator('.silences select').selectOption('2');
      await page.locator('.pause-mark').first().waitFor();
      assert.equal(await page.locator('.pause-mark').count(), 2);
      assert.equal(await page.locator('.timeline .quiet').count(), 2);
      await page.locator('.pause-mark').first().click();
      await page.waitForFunction(() => (window.fixture.saves.at(-1)?.removed ?? []).length === 1);
      const [a, b] = await page.evaluate(() => window.fixture.saves.at(-1).removed[0]);
      assert.ok(Math.abs(a - 50.25) < 0.04 && Math.abs(b - 53.75) < 0.04, `kept 0.25 s each side, got ${a}-${b}`);
      await page.locator('.pause-mark.gone').first().waitFor();
      await page.getByRole('button', { name: 'Ta bort alla tystnader' }).click();
      await page.waitForFunction(() => (window.fixture.saves.at(-1)?.removed ?? []).length === 2);
      await page.locator('.pause-mark.gone').first().click();
      await page.waitForFunction(() => (window.fixture.saves.at(-1)?.removed ?? []).length === 1);
      await page.keyboard.press('Control+z'); await page.keyboard.press('Control+z'); await page.keyboard.press('Control+z');
      await page.waitForFunction(() => { const s = window.fixture.saves.at(-1); return !s.removed.length && !s.kept.length; });
      await page.evaluate(() => { window.fixture.silences = []; });
      await page.locator('.silences select').selectOption('3');
    });
    await step('retakes are marked in the text and removed from there', async () => {
      const chips = page.locator('.retake-mark');
      assert.ok(await chips.count() >= 2, 'a ↺ before each earlier take');
      const before = await page.evaluate(() => window.fixture.saves.at(-1)?.deleted.length ?? 0);
      await chips.first().click();
      await page.waitForFunction(n => (window.fixture.saves.at(-1)?.deleted.length ?? 0) > n, before);
      await page.locator('.retake-mark.gone').first().waitFor();
      await page.keyboard.press('Control+z');
      await page.waitForFunction(n => (window.fixture.saves.at(-1)?.deleted.length ?? 0) === n, before);
    });
    await step('listening to a join loops around it and never plays the struck word', async () => {
      const w = await page.evaluate(id => window.fixture.project.transcript.utterances.flatMap(u => u.words)[id], egentligen);
      await page.evaluate(t => { document.querySelector('video').currentTime = t - 0.6; }, w.start);
      await page.waitForTimeout(150);
      await page.getByRole('button', { name: 'Lyssna på skarven' }).click();
      const seen = await page.evaluate(() => new Promise(done => {
        const v = document.querySelector('video'), times = []; const until = performance.now() + 4500;
        (function tick() { times.push(v.currentTime); if (performance.now() < until) requestAnimationFrame(tick); else done(times); })();
      }));
      assert.ok(!seen.some(t => t > w.start + 0.05 && t < w.end - 0.05), 'struck word played while looping');
      const jumpsBack = seen.filter((t, i) => i && t < seen[i - 1] - 0.5).length;
      assert.ok(jumpsBack >= 1, 'the loop restarts');
      await page.getByRole('button', { name: 'Sluta lyssna' }).click();
      await page.screenshot({ path: `${shots}/editor-detail.png` });
    });    await step('current word is highlighted while playing', async () => {
      assert.equal(await page.locator('.doc .current').count(), 1);
    });
    await step('playhead latency compensation: adjustable, remembered, only while playing', async () => {
      const slider = page.getByLabel(/Markörens synk mot ljudet/);
      await slider.fill('200');
      await page.getByText('200 ms').waitFor();
      assert.equal(await page.evaluate(() => localStorage.getItem('textklipp.latencyMs')), '200');
      await page.getByRole('button', { name: 'Spela' }).click();
      await page.waitForTimeout(800);
      const lag = await page.evaluate(() => {
        const v = document.querySelector('video'), bar = document.querySelector('.timeline'), head = document.querySelector('.timeline .playhead');
        const shown = (parseFloat(head.style.left) / 100) * window.fixture.project.media.duration;
        return v.currentTime - shown;
      });
      assert.ok(lag > 0.12 && lag < 0.3, `playhead should trail media time by ~0.2 s, trails ${lag}`);
      await page.getByRole('button', { name: 'Pausa' }).click();
      await page.getByRole('button', { name: /Mät automatiskt/ }).click();
      assert.equal(await page.evaluate(() => localStorage.getItem('textklipp.latencyMs')), null);
    });    await step('hiding struck text and shortening pauses', async () => {
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
    await step('export dialog: options, saves edits first, reports sync, reveals the file', async () => {
      await page.getByRole('button', { name: 'Exportera…' }).click();
      const dialog = page.getByRole('dialog', { name: /Exportera klippt film/ });
      await dialog.waitFor();
      await dialog.getByLabel('Undertexter (.srt)').check();
      await dialog.getByLabel('Mindre fil').check();
      await dialog.getByRole('button', { name: 'Välj plats och exportera…' }).click();
      await dialog.getByText('Bild och ljud kontrollerade').waitFor();
      const ex = await page.evaluate(() => window.fixture.exported);
      assert.equal(ex.args.quality, 'small'); assert.equal(ex.args.srt, true); assert.equal(ex.args.vtt, false);
      assert.match(await page.evaluate(() => window.fixture.saveDialog.options.defaultPath), /Testklipp \(klippt\)\.mp4$/);
      await dialog.getByText('Testklipp (klippt).srt').waitFor();
      await dialog.getByRole('button', { name: 'Visa i mappen' }).click();
      await page.waitForFunction(() => !!window.fixture.revealed);
      await page.screenshot({ path: `${shots}/export-done.png` });
      await dialog.getByRole('button', { name: 'Stäng', exact: true }).last().click();
      await dialog.waitFor({ state: 'hidden' });
    });    await step('back to the list saves and closes', async () => {
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
