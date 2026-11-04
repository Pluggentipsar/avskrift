<script lang="ts">
  import { summarizeSpeakers, type SpeakerUtterance } from './speakers';
  // Speakers in the transcript: listen to each one, give them a name from the participant list,
  // merge two that are the same person, and (for meetings) split "Mötet" into the participants.
  let { utterances, speakerLabels, participantNames = [], canSeparate = false, separateCount = $bindable(null), open = $bindable(false), busy = false, working = false, readonly = false,
    onrename, onmerge, onlisten, onseparate }: {
    utterances: SpeakerUtterance[]; speakerLabels: Record<string, string>; participantNames?: string[];
    canSeparate?: boolean; separateCount?: number | null; open?: boolean; busy?: boolean; working?: boolean; readonly?: boolean;
    onrename: (id: string, name: string) => void; onmerge: (from: string, into: string) => void;
    onlisten: (start: number, end: number) => void; onseparate: (count: number | null) => void;
  } = $props();

  const speakers = $derived(summarizeSpeakers(utterances));
  const split = $derived(speakers.some((s) => s.id.startsWith('TALARE_')));
  const name = (id: string) => speakerLabels[id] ?? id;
  const fmt = (s: number) => (s < 60 ? `${Math.round(s)} s` : `${Math.round(s / 60)} min`);
  const counts = Array.from({ length: 12 }, (_, i) => i + 1);
</script>

<details class="speaker-panel" bind:open>
  <summary>
    <strong>Talare</strong>
    <span>{speakers.map((s) => name(s.id)).join(' · ')}</span>
  </summary>
  {#if working}<p class="working" role="status">Skiljer på deltagarna i bakgrunden. Det tar några minuter för ett långt möte, och du kan fortsätta arbeta så länge.</p>{/if}
  {#if canSeparate && !readonly}
    <div class="separate">
      <label>Övriga deltagare (utom dig)
        <select aria-label="Antal röster i mötet" value={separateCount ?? ''} onchange={(e) => (separateCount = e.currentTarget.value ? +e.currentTarget.value : null)}>
          <option value="">Gissa</option>
          {#each counts as n}<option value={n}>{n}</option>{/each}
        </select>
      </label>
      <button class="primary" onclick={() => onseparate(separateCount)} disabled={busy}>{split ? 'Skilj på deltagarna igen' : 'Skilj på deltagarna'}</button>
      <p class="hint">Delar upp det som hörs från mötet efter röst. Ange hur många som pratade, så blir det träffsäkrare. Din röst förblir ”Jag”.{split ? ' Görs det om försvinner namnen på Talare 1, 2 …' : ''}</p>
    </div>
  {/if}
  <datalist id="speaker-names">{#each participantNames as p}<option value={p}></option>{/each}</datalist>
  <ul>
    {#each speakers as s (s.id)}
      <li>
        <button class="listen" onclick={() => onlisten(s.sample.start, s.sample.end)} aria-label={`Lyssna på ${name(s.id)}`} title={s.sample.text}>▶</button>
        <input aria-label={`Namn på ${name(s.id)}`} list="speaker-names" value={name(s.id)} disabled={readonly}
          onchange={(e) => onrename(s.id, e.currentTarget.value.trim() || s.id)} />
        <span class="stats">{s.count} inlägg · {fmt(s.seconds)}</span>
        {#if speakers.length > 1 && !readonly}
          <select aria-label={`Slå ihop ${name(s.id)} med`} value="" onchange={(e) => { const into = e.currentTarget.value; e.currentTarget.value = ''; if (into) onmerge(s.id, into); }}>
            <option value="">Slå ihop med…</option>
            {#each speakers.filter((o) => o.id !== s.id) as o}<option value={o.id}>{name(o.id)}</option>{/each}
          </select>
        {/if}
      </li>
    {/each}
  </ul>
  {#if participantNames.length && !readonly}<p class="hint">Skriv eller välj ett namn ur deltagarlistan. Namnet ändras i hela transkriptet.</p>{/if}
</details>

<style>
  .speaker-panel{margin:0 0 14px;border:1px solid var(--line);border-radius:8px;background:var(--bg);font:14px/1.45 Archivo,sans-serif;color:var(--ink)}
  summary{display:flex;gap:12px;align-items:baseline;padding:10px 14px;cursor:pointer;list-style:none}
  summary::-webkit-details-marker{display:none}
  summary::before{content:'▸';color:var(--muted)}
  details[open] summary::before{content:'▾'}
  summary span{color:var(--muted);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;min-width:0}
  .separate{display:flex;flex-wrap:wrap;gap:10px 14px;align-items:end;padding:4px 14px 12px;border-bottom:1px solid var(--line)}
  .separate label{display:flex;flex-direction:column;gap:4px;font-size:13px;color:var(--muted)}
  .separate .hint{flex-basis:100%}
  ul{list-style:none;margin:0;padding:8px 14px}
  li{display:flex;gap:10px;align-items:center;padding:5px 0;flex-wrap:wrap}
  input{flex:1 1 180px;min-width:0;padding:6px 9px;border:1px solid var(--line-2);border-radius:6px;font:inherit;background:var(--bg);color:var(--ink)}
  select,button{padding:6px 10px;border:1px solid var(--line-2);border-radius:6px;background:var(--bg);color:var(--ink);font:inherit;cursor:pointer}
  button:disabled{opacity:.5;cursor:default}
  .primary{background:var(--accent);border-color:var(--accent);color:#fff}
  .listen{width:34px;padding:6px 0}
  .stats{color:var(--muted);font-size:13px;white-space:nowrap}
  .working{margin:0;padding:4px 14px 12px;color:var(--accent)}
  .hint{margin:0;color:var(--muted);font-size:13px}
  .speaker-panel > .hint{padding:0 14px 12px}
  button:focus-visible,select:focus-visible,input:focus-visible,summary:focus-visible{outline:2px solid var(--accent);outline-offset:2px}
</style>
