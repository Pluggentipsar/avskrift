<script lang="ts">
  import { onMount, tick } from 'svelte';
  import TranscriptBlock from './TranscriptBlock.svelte';
  import { transcriptBlocks, playbackIndex, matchingSegments, type Utterance } from './transcript-view';
  let { utterances, speakerLabels, speakerOptions, playing, currentTime, editMode, editingIdx, editText = $bindable(''),
    onseek, onedit, oncommit, oncancel, onrename, onspeaker, ondelete }: {
    utterances: Utterance[]; speakerLabels: Record<string,string>; speakerOptions: string[]; playing: boolean; currentTime: number;
    editMode: boolean; editingIdx: number|null; editText?: string;
    onseek: (time:number)=>void; onedit:(i:number)=>void; oncommit:()=>void; oncancel:()=>void;
    onrename:(id:string,name:string)=>void; onspeaker:(i:number,speaker:string)=>void; ondelete:(i:number)=>void;
  } = $props();
  let root = $state<HTMLDivElement>(), searchInput: HTMLInputElement;
  let query = $state(''), hit = $state(0), follow = $state(true), fontSize = $state(16), width = $state(700);
  let revealed = $state(0), focusedRow = $state(0);
  const blocks = $derived(transcriptBlocks(utterances));
  const texts = $derived(utterances.map(u => u.text.toLocaleLowerCase('sv')));
  const matches = $derived(matchingSegments(texts, query));
  const matchIndex = $derived(matches[Math.min(hit, Math.max(0,matches.length - 1))] ?? -1);
  const lookup = $derived(playbackIndex(utterances));
  const active = $derived(playing ? lookup(currentTime) : -1);
  const time = (s:number) => `${Math.floor(s/60)}:${Math.floor(s%60).toString().padStart(2,'0')}`;
  onMount(() => {
    try { const saved = Number(localStorage.getItem('avskrift.readingSize')); if ([14,16,19,22].includes(saved)) fontSize = saved; } catch {}
    const observer = new ResizeObserver(() => {
      if (root && width!==root.clientWidth) {
        const anchor=follow&&active>=0?active:navigating?revealed:readingIndex();
        width=root.clientWidth;void reveal(anchor);
      }
    });
    if (root) observer.observe(root);
    return () => observer.disconnect();
  });
  function readingIndex() {
    if(!root)return revealed;
    const top=root.getBoundingClientRect().top;
    const row=Array.from(root.querySelectorAll<HTMLElement>('[data-segment]')).find(el=>el.getBoundingClientRect().bottom>top+16);
    return row ? Number(row.dataset.segment) : revealed;
  }
  function resizeText(value:number) {
    const anchor=follow&&active>=0?active:readingIndex();
    fontSize=value;try {localStorage.setItem('avskrift.readingSize',String(value));} catch {}
    void reveal(anchor);
  }
  let navigation = 0, navigating = false;
  export async function reveal(index:number, focus=false) {
    if (!root || index < 0 || index >= utterances.length) return;
    const revision = ++navigation;
    navigating = true;
    revealed = index;
    // Revealing neighboring blocks can change their estimated heights. Keep the requested
    // segment anchored during this short layout cycle, including after font/width changes.
    for(let pass=0;pass<5;pass++) {
      await tick();
      await new Promise<void>(resolve=>requestAnimationFrame(()=>resolve()));
      if (revision !== navigation || !root) return;
      const row = root.querySelector<HTMLElement>(`[data-segment="${index}"]`);
      if (!row) break;
      const bounds = row.getBoundingClientRect(), viewport = root.getBoundingClientRect();
      if (bounds.top < viewport.top + 16 || bounds.bottom > viewport.bottom - 16) {
        root.scrollTop += bounds.top - viewport.top - Math.max(16,(root.clientHeight - bounds.height)/2);
      }
      if (focus && pass===0) { focusedRow=index; row.querySelector<HTMLElement>('.body')?.focus({preventScroll:true}); }
    }
    navigating = false;
  }
  $effect(() => { if (follow && active >= 0 && editingIdx === null) void reveal(active); });
  $effect(() => { void query; if (matchIndex >= 0) { follow=false; void reveal(matchIndex); } });
  function nextMatch(direction:number) {
    if(!matches.length)return;
    hit=(hit+direction+matches.length)%matches.length;
    follow=false;void reveal(matches[hit]);
  }
  function stopFollowing() { follow=false;++navigation;navigating=false; }
  function beginEdit(index:number) { stopFollowing();onedit(index); }
  function rowKey(event:KeyboardEvent,index:number) {
    if (event.target !== event.currentTarget) return;
    if (event.key==='Enter'||event.key==='F2') {event.preventDefault();beginEdit(index);return;}
    let next=index;
    if(event.key==='ArrowDown')next++;
    else if(event.key==='ArrowUp')next--;
    else if((event.ctrlKey||event.metaKey)&&event.key==='Home')next=0;
    else if((event.ctrlKey||event.metaKey)&&event.key==='End')next=utterances.length-1;
    else return;
    event.preventDefault();follow=false;void reveal(Math.max(0,Math.min(utterances.length-1,next)),true);
  }
  function focusEditor(node:HTMLTextAreaElement) { node.focus({preventScroll:true}); node.style.height=`${Math.max(90,node.scrollHeight)}px`; }
  async function finishEdit(cancel=false) {const index=editingIdx;cancel?oncancel():oncommit();await tick();if(index!==null)void reveal(index,true);}
  function findShortcut(event:KeyboardEvent) {
    if((event.ctrlKey||event.metaKey)&&event.key.toLowerCase()==='f' && root?.getClientRects().length) {
      const target=event.target as HTMLElement;
      if(target.closest('dialog'))return;
      event.preventDefault();searchInput.focus();searchInput.select();
    }
  }
</script>
<svelte:window onkeydown={findShortcut} />
<div class="reading-tools" role="search" aria-label="Sök i hela transkriptet">
  <input bind:this={searchInput} type="search" aria-label="Sök i transkript" placeholder="Sök i hela transkriptet…" bind:value={query} oninput={()=>hit=0}
    onkeydown={e=>{if(e.key==='Enter'){e.preventDefault();nextMatch(e.shiftKey?-1:1);}}} />
  <span role="status">{query.trim() ? matches.length ? `${Math.min(hit+1,matches.length)} av ${matches.length} avsnitt` : 'Inga träffar' : `${utterances.length} avsnitt`}</span>
  <button aria-label="Föregående träff" onclick={()=>nextMatch(-1)} disabled={!matches.length}>↑</button>
  <button aria-label="Nästa träff" onclick={()=>nextMatch(1)} disabled={!matches.length}>↓</button>
  <label class="size">Textstorlek<select aria-label="Textstorlek" value={fontSize} onchange={e=>resizeText(+e.currentTarget.value)}><option value={14}>Liten</option><option value={16}>Normal</option><option value={19}>Stor</option><option value={22}>Extra stor</option></select></label>
</div>
<div class="reading-options"><label><input type="checkbox" bind:checked={follow} /> Följ uppspelningen</label><span>Piltangenter byter avsnitt. Enter rättar texten.</span></div>
<!-- Scroll containers must be keyboard-focusable, including when the focused row has been virtualized. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div class="transcript" bind:this={root} style:--reading-size={`${fontSize}px`} aria-label="Transkriptets avsnitt" role="region" tabindex="0"
  onwheel={stopFollowing} ontouchmove={stopFollowing} onkeydown={e=>{if(['PageUp','PageDown','Home','End','ArrowUp','ArrowDown'].includes(e.key))follow=false;if(e.target===root)rowKey(e,focusedRow);}}>
  {#if root}
    {#each blocks as block, b (block.start)}
      <TranscriptBlock {root} index={b} forced={(revealed>=block.start&&revealed<block.end)||(editingIdx!==null&&editingIdx>=block.start&&editingIdx<block.end)}
        estimate={Math.ceil(block.characters/Math.max(20,(width-64)/(fontSize*.55)))*fontSize*1.8+(block.end-block.start)*34}>
        {#each utterances.slice(block.start,block.end) as u, offset (block.start+offset)}
          {@const i=block.start+offset}
          <div class="turn" data-segment={i} class:match={matchIndex===i}>
            {#if u.speaker && (offset===0 || utterances[i-1].speaker!==u.speaker)}
              <input class="speaker" aria-label={`Talarnamn ${speakerLabels[u.speaker]??u.speaker}`} value={speakerLabels[u.speaker]??u.speaker} oninput={e=>onrename(u.speaker!,e.currentTarget.value)} />
            {/if}
            <div class="utext">
              <button class="ts" onclick={()=>onseek(u.start)} aria-label={`Spela från ${time(u.start)}`}>{time(u.start)}</button>
              {#if editingIdx===i}<textarea class="edit" aria-label="Redigera avsnitt" use:focusEditor bind:value={editText} onblur={oncommit}
                onkeydown={e=>{if(e.key==='Escape'){e.preventDefault();void finishEdit(true);}if(e.key==='Enter'&&(e.ctrlKey||e.metaKey)){e.preventDefault();void finishEdit();}}}></textarea>
              {:else}<div class="body" class:editing={editMode} role="button" tabindex={focusedRow===i?0:-1} aria-label={`Avsnitt ${i+1}: ${u.text}`}
                onfocus={()=>focusedRow=i} onclick={()=>{if(editMode)beginEdit(i);}} ondblclick={()=>beginEdit(i)} onkeydown={e=>rowKey(e,i)}>
                {#if u.words?.length}{#each u.words as word}<button class="word" tabindex="-1" class:playing={playing&&currentTime>=word.start&&currentTime<word.end}
                  onclick={e=>{e.stopPropagation();editMode?beginEdit(i):onseek(word.start);}}>{word.text}</button>{' '}{/each}
                {:else}<span class="useg" class:playing={active===i}>{u.text}</span>{/if}
              </div>{/if}
              {#if editMode}<span class="ed-ctrls">{#if speakerOptions.length}<select aria-label={`Talare för avsnitt ${i+1}`} value={u.speaker??''} onchange={e=>onspeaker(i,e.currentTarget.value)}><option value="">Ingen talare</option>{#each speakerOptions as s}<option value={s}>{speakerLabels[s]??s}</option>{/each}</select>{/if}<button aria-label={`Ta bort avsnitt ${i+1}`} onclick={()=>ondelete(i)}>×</button></span>{/if}
            </div>
          </div>
        {/each}
      </TranscriptBlock>
    {/each}
  {/if}
</div>
<style>
  .reading-tools{display:flex;align-items:center;gap:8px;flex-wrap:wrap;margin-bottom:10px;font-size:13px}.reading-tools>input{flex:1;min-width:160px}.reading-tools>span{color:var(--muted);font-variant-numeric:tabular-nums}.size{display:flex;align-items:center;gap:8px;margin-left:auto}.reading-options{display:flex;justify-content:space-between;gap:12px;flex-wrap:wrap;color:var(--muted);font-size:12px;margin-bottom:12px}.reading-options label{display:flex;gap:6px;align-items:center}
  input,button,select{font:inherit;color:var(--ink);background:var(--bg);border:1px solid var(--line-2);border-radius:6px;padding:8px}button{cursor:pointer}button:disabled{opacity:.45}
  .transcript{flex:1;min-height:220px;overflow:auto;max-height:70dvh;width:100%;max-width:88ch;box-sizing:border-box;align-self:center;background:var(--bg);border:1px solid var(--line);border-radius:8px;padding:16px 20px;scroll-behavior:auto;overscroll-behavior:contain;scrollbar-gutter:stable}
  .turn{padding:6px 8px;border-left:3px solid transparent;overflow-wrap:anywhere}.turn.match{border-left-color:var(--accent);background:var(--accent-soft)}.speaker{font:600 13px Archivo,sans-serif;color:var(--accent);background:none;border:0;padding:2px 0;max-width:100%}.utext{line-height:1.8;font-size:var(--reading-size);display:flex;align-items:baseline;gap:10px;flex-wrap:wrap}.ts{font-size:11px;font-variant-numeric:tabular-nums;color:var(--muted);padding:2px 0;border:0;background:none;flex:none}.body{flex:1;min-width:100px;cursor:text;border-radius:3px}.body.editing:hover{background:var(--accent-soft)}.word,.useg{border:0;background:none;font:inherit;line-height:inherit;color:inherit;padding:0 1px;border-radius:3px}.word:hover,.word.playing,.useg.playing{background:var(--accent-soft);color:var(--accent)}.edit{flex:1;min-width:150px;box-sizing:border-box;font:inherit;line-height:1.7;color:var(--ink);background:var(--bg);border:1px solid var(--accent);border-radius:6px;padding:8px;resize:vertical}.ed-ctrls{display:flex;gap:6px;font-size:12px;align-items:center}.ed-ctrls select{max-width:150px}
  :is(input,button,select,textarea,.body,.transcript):focus-visible{outline:2px solid var(--accent);outline-offset:2px}
  @media(max-width:760px){.transcript{padding:12px 8px;height:55dvh;flex:auto}.reading-options>span{display:none}.size{margin-left:0}}
</style>
