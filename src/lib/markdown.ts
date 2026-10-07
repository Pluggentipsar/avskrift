// Markdown in summaries: show it formatted, keep structure when formatted text (e.g. from an AI chat
// in the browser) is pasted, and copy it so it looks right in both Word and another chat.
import { marked } from 'marked';
import DOMPurify from 'dompurify';

/** Markdown to HTML that is safe to insert: pasted text may contain markup, never let it run. */
export function renderMarkdown(md: string): string {
  const html = marked.parse(md, { gfm: true, breaks: false, async: false }) as string;
  return DOMPurify.sanitize(html, { USE_PROFILES: { html: true }, FORBID_TAGS: ['style', 'img', 'iframe', 'form', 'input'] });
}

/** Whether pasted HTML carries structure worth converting (otherwise the plain text is better). */
export function hasStructure(html: string): boolean {
  return /<(table|h[1-6]|ul|ol|strong|b|em|i|blockquote|pre)[\s>]/i.test(html);
}

const BLOCKS = new Set(['P', 'DIV', 'SECTION', 'ARTICLE', 'HEADER', 'FOOTER', 'MAIN']);

/** Formatted text from the clipboard (HTML) to markdown: headings, lists, emphasis, links, code,
 *  quotes and tables. Unknown elements keep their text. */
export function htmlToMarkdown(html: string): string {
  const doc = new DOMParser().parseFromString(html, 'text/html');
  doc.querySelectorAll('script,style,meta,link,title,button,svg').forEach(e => e.remove());
  const out = blocks(doc.body, 0).replace(/\n{3,}/g, '\n\n').trim();
  return out + '\n';
}

function blocks(node: Node, depth: number): string {
  let out = '', inline = '';
  const flush = () => { if (inline.trim()) out += inline.replace(/[ \t]+\n/g, '\n').trim() + '\n\n'; inline = ''; };
  for (const child of Array.from(node.childNodes)) {
    if (child.nodeType === Node.TEXT_NODE) { inline += collapse(child.textContent ?? ''); continue; }
    if (!(child instanceof Element)) continue;
    const tag = child.tagName;
    if (/^H[1-6]$/.test(tag)) { flush(); out += '#'.repeat(Number(tag[1])) + ' ' + inlines(child).trim() + '\n\n'; }
    else if (tag === 'UL' || tag === 'OL') { flush(); out += list(child, depth) + '\n'; }
    else if (tag === 'TABLE') { flush(); out += table(child) + '\n\n'; }
    else if (tag === 'PRE') { flush(); out += '```\n' + (child.textContent ?? '').replace(/\n$/, '') + '\n```\n\n'; }
    else if (tag === 'BLOCKQUOTE') { flush(); out += blocks(child, depth).trim().split('\n').map(l => '> ' + l).join('\n') + '\n\n'; }
    else if (tag === 'HR') { flush(); out += '---\n\n'; }
    else if (tag === 'BR') { inline += '\n'; }
    else if (BLOCKS.has(tag)) { flush(); out += blocks(child, depth); }
    else { inline += inlineOf(child); }
  }
  flush();
  return out;
}

function collapse(text: string): string {
  return text.replace(/\s+/g, ' ');
}

function inlines(node: Node): string {
  let s = '';
  for (const child of Array.from(node.childNodes)) {
    if (child.nodeType === Node.TEXT_NODE) s += collapse(child.textContent ?? '');
    else if (child instanceof Element) s += inlineOf(child);
  }
  return s;
}

function inlineOf(el: Element): string {
  const inner = inlines(el);
  const wrap = (mark: string) => (inner.trim() ? mark + inner.trim() + mark + (inner.endsWith(' ') ? ' ' : '') : inner);
  switch (el.tagName) {
    case 'STRONG': case 'B': return wrap('**');
    case 'EM': case 'I': return wrap('*');
    case 'CODE': return '`' + (el.textContent ?? '') + '`';
    case 'BR': return '\n';
    case 'A': {
      const href = el.getAttribute('href') ?? '';
      return /^https?:/i.test(href) && inner.trim() && inner.trim() !== href ? `[${inner.trim()}](${href})` : inner;
    }
    default: return inner;
  }
}

function list(el: Element, depth: number): string {
  const ordered = el.tagName === 'OL';
  let n = Number(el.getAttribute('start') ?? 1) || 1;
  let out = '';
  for (const li of Array.from(el.children).filter(c => c.tagName === 'LI')) {
    const nested = Array.from(li.children).filter(c => c.tagName === 'UL' || c.tagName === 'OL');
    const clone = li.cloneNode(true) as Element;
    clone.querySelectorAll(':scope > ul, :scope > ol').forEach(e => e.remove());
    const text = blocks(clone, depth + 1).trim().replace(/\n+/g, ' ');
    out += '  '.repeat(depth) + (ordered ? `${n++}. ` : '- ') + text + '\n';
    for (const sub of nested) out += list(sub, depth + 1);
  }
  return out;
}

function cell(el: Element): string {
  return inlines(el).replace(/\s*\n\s*/g, ' ').trim().replace(/\|/g, '\\|');
}

function table(el: Element): string {
  const rows = Array.from(el.querySelectorAll('tr')).map(tr => Array.from(tr.children).filter(c => c.tagName === 'TD' || c.tagName === 'TH').map(cell));
  if (!rows.length) return '';
  const width = Math.max(...rows.map(r => r.length));
  const pad = (r: string[]) => [...r, ...Array(width - r.length).fill('')];
  const line = (r: string[]) => '| ' + pad(r).join(' | ') + ' |';
  return [line(rows[0]), line(Array(width).fill('---')), ...rows.slice(1).map(line)].join('\n');
}

/** Copy markdown so it pastes as formatting in Word and as markdown in plain-text places. */
export async function copyMarkdown(md: string): Promise<void> {
  try {
    const html = `<meta charset="utf-8">${renderMarkdown(md)}`;
    await navigator.clipboard.write([new ClipboardItem({
      'text/plain': new Blob([md], { type: 'text/plain' }),
      'text/html': new Blob([html], { type: 'text/html' }),
    })]);
  } catch {
    await navigator.clipboard.writeText(md);
  }
}

/** Read the clipboard as markdown: formatted text is converted, plain text kept as it is. */
export async function readClipboardMarkdown(): Promise<string> {
  try {
    for (const item of await navigator.clipboard.read()) {
      if (item.types.includes('text/html')) {
        const html = await (await item.getType('text/html')).text();
        if (hasStructure(html)) return htmlToMarkdown(html);
      }
      if (item.types.includes('text/plain')) return await (await item.getType('text/plain')).text();
    }
  } catch { /* fall back to plain text */ }
  return await navigator.clipboard.readText();
}
