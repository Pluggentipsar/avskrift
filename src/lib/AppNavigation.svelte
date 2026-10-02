<script lang="ts">
  import { ICONS } from '$lib/icons';
  let { active, meetingActive, dictationActive, overdue, version, speechModel = '', onnavigate, onnew, onmodels }: {
    active: string; meetingActive: boolean; dictationActive: boolean; overdue: number; version: string; speechModel?: string;
    onnavigate: (page: string) => void; onnew: () => void; onmodels: () => void;
  } = $props();
  type Item = { id: string; label: string; icon: string };
  // Grouped by what you do: make something, work on text, find it again.
  const groups: { label: string; items: Item[] }[] = [
    { label: '', items: [{ id: 'home', label: 'Ditt arbete', icon: ICONS.home }] },
    { label: 'Skapa', items: [
      { id: 'meeting', label: 'Möten', icon: ICONS.meeting },
      { id: 'transcribe', label: 'Transkribera', icon: ICONS.transcribe },
      { id: 'dictation', label: 'Diktering', icon: ICONS.dictation },
      { id: 'textklipp', label: 'Textklipp', icon: ICONS.textklipp },
    ] },
    { label: 'Bearbeta', items: [
      { id: 'deidentify', label: 'Avidentifiering', icon: ICONS.deidentify },
      { id: 'summarize', label: 'Sammanfatta text', icon: ICONS.summarize },
    ] },
    { label: 'Hitta', items: [
      { id: 'history', label: 'Bibliotek', icon: ICONS.history },
      { id: 'tasks', label: 'Åtaganden', icon: ICONS.tasks },
    ] },
  ];
  const busy = (id: string) => (id === 'meeting' && meetingActive) || (id === 'dictation' && dictationActive);
</script>

<aside class="app-navigation">
  <button class="brand" onclick={() => onnavigate('home')} aria-label="Avskrift – ditt arbete">Avskrift<span>Ord att arbeta vidare med.</span></button>
  <button class="new" onclick={onnew}>
    <svg viewBox="0 0 24 24" aria-hidden="true"><path d={ICONS.plus} /></svg>Nytt arbete
  </button>
  <nav aria-label="Huvudnavigation">
    {#each groups as group}
      <div class="group">
        {#if group.label}<div class="group-label">{group.label}</div>{/if}
        {#each group.items as item}
          <button aria-current={active === item.id ? 'page' : undefined} onclick={() => onnavigate(item.id)}>
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d={item.icon} /></svg>
            <span class="label">{item.label}</span>
            {#if busy(item.id)}<span class="activity" aria-label="Pågår"></span>{/if}
            {#if item.id === 'tasks' && overdue}<span class="count" aria-label="{overdue} försenade">{overdue}</span>{/if}
          </button>
        {/each}
      </div>
    {/each}
  </nav>
  <div class="status">
    <div class="local"><span class="dot"></span>Allt körs lokalt</div>
    {#if speechModel}<div class="model">Talmodell: {speechModel}</div>{/if}
    <button class="link" onclick={onmodels}>Modeller på datorn</button>
  </div>
  {#if version}<p class="version">Version {version}</p>{/if}
</aside>

<style>
  .app-navigation { background:var(--nav-bg); border-right:1px solid var(--line); padding:28px 16px 18px; display:flex; flex-direction:column; gap:20px; min-width:0; overflow:auto; }
  /* A low window scrolls the menu instead of squeezing its parts (the button collapsed at 720 px). */
  .app-navigation > *, nav button { flex-shrink:0; }
  button { font:inherit; cursor:pointer; color:var(--ink); border:0; background:transparent; text-align:left; }
  svg { width:18px; height:18px; flex:none; fill:none; stroke:currentColor; stroke-width:1.7; stroke-linecap:round; stroke-linejoin:round; }
  .brand { font:34px/1 'Instrument Serif',serif; padding:0 12px; color:var(--ink); }
  .brand span { display:block; font:12px/1.5 Archivo,sans-serif; color:var(--muted); margin-top:10px; }
  .new { display:flex; align-items:center; justify-content:center; gap:8px; height:42px; border-radius:8px; background:var(--accent); color:#fff; font-weight:600; }
  .new:hover { background:var(--accent-press); }
  nav { display:flex; flex-direction:column; gap:16px; }
  .group { display:flex; flex-direction:column; gap:2px; }
  .group-label { font-size:11px; font-weight:600; letter-spacing:.06em; text-transform:uppercase; color:var(--muted); padding:0 12px 4px; }
  nav button { display:flex; align-items:center; gap:10px; height:38px; padding:0 12px; border-radius:6px; font-size:14px; color:#3b3c38; }
  nav button:hover { background:#e8e8e3; color:var(--ink); }
  nav button[aria-current=page] { background:var(--accent-soft); color:var(--accent); font-weight:600; }
  .label { flex:1; min-width:0; }
  .activity { width:7px; height:7px; border-radius:50%; background:var(--accent); }
  .count { min-width:20px; height:20px; border-radius:10px; background:#f6e2d3; color:#8a3b0c; font-size:11px; font-weight:600; display:grid; place-items:center; }
  .status { margin-top:auto; border:1px solid var(--line); border-radius:8px; background:var(--bg); padding:12px 14px; display:flex; flex-direction:column; gap:4px; font-size:12px; color:var(--muted); }
  .local { display:flex; align-items:center; gap:8px; font-size:13px; font-weight:600; color:var(--ink); }
  .dot { width:8px; height:8px; border-radius:50%; background:#2e7d50; }
  .link { color:var(--ink); text-decoration:underline; text-underline-offset:3px; font-size:12px; padding:0; align-self:flex-start; }
  .version { margin:0; padding:0 12px; font-size:11px; color:var(--muted); }
  :is(button):focus-visible { outline:2px solid var(--accent); outline-offset:2px; }
  @media(max-width:1050px) {
    .app-navigation { padding:12px 20px; flex-direction:row; align-items:center; flex-wrap:wrap; gap:12px; border-right:0; border-bottom:1px solid var(--line); overflow:visible; }
    .brand { font-size:28px; padding:0; } .brand span, .status, .version, .group-label { display:none; }
    .new { height:36px; padding:0 12px; }
    nav { flex:1 1 100%; min-width:0; flex-direction:row; flex-wrap:wrap; gap:4px; } .group { flex-direction:row; flex-wrap:wrap; }
    nav button { height:34px; padding:0 9px; }
  }
</style>
