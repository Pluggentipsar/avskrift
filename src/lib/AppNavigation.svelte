<script lang="ts">
  let { active, meetingActive, dictationActive, overdue, version, onnavigate, onnew, onmodels }: {
    active: string; meetingActive: boolean; dictationActive: boolean; overdue: number; version: string;
    onnavigate: (page: string) => void; onnew: () => void; onmodels: () => void;
  } = $props();
  const destinations = [
    { id: 'home', label: 'Ditt arbete' }, { id: 'meeting', label: 'Möten' },
    { id: 'dictation', label: 'Diktering' }, { id: 'deidentify', label: 'Avidentifiering' },
  ];
</script>

<aside class="app-navigation">
  <button class="brand" onclick={() => onnavigate('home')} aria-label="AVskrift – ditt arbete">Avskrift<span>Ord att arbeta vidare med.</span></button>
  <nav aria-label="Huvudnavigation">
    {#each destinations as item}
      <button aria-current={active === item.id ? 'page' : undefined} onclick={() => onnavigate(item.id)}>
        {item.label}
        {#if (item.id === 'meeting' && meetingActive) || (item.id === 'dictation' && dictationActive)}<span class="activity" aria-label="Pågår"></span>{/if}
      </button>
    {/each}
  </nav>
  <nav class="secondary" aria-label="Bibliotek och verktyg">
    <button aria-current={active === 'history' ? 'page' : undefined} onclick={() => onnavigate('history')}>Bibliotek</button>
    <button aria-current={active === 'tasks' ? 'page' : undefined} onclick={() => onnavigate('tasks')}>Åtaganden {#if overdue}<span class="count">{overdue}</span>{/if}</button>
    <button onclick={onmodels}>Modeller på datorn</button>
    <button aria-current={active === 'summarize' ? 'page' : undefined} onclick={() => onnavigate('summarize')}>Sammanfatta text</button>
  </nav>
  <div class="bottom"><button class="new" onclick={onnew}>Nytt arbete</button><p>Bearbetas på din dator<br />{#if version}<span>Version {version} · mallflöde</span>{/if}</p></div>
</aside>

<style>
  .app-navigation { background:var(--nav-bg); border-right:1px solid var(--line); padding:30px 16px 18px; display:flex; flex-direction:column; gap:28px; min-width:0; overflow:auto; }
  button { font:inherit; cursor:pointer; color:var(--muted); border:0; background:transparent; text-align:left; }
  .brand { font:36px/1 'Instrument Serif',serif; padding:0 12px; color:var(--ink); }
  .brand span { display:block; font:12px/1.5 Archivo,sans-serif; color:var(--muted); margin-top:12px; }
  nav { display:grid; gap:5px; } nav button { padding:12px; border-radius:8px; font-size:14px; display:flex; align-items:center; gap:8px; }
  nav button:hover { background:var(--accent-soft); color:var(--ink); }
  nav button[aria-current=page] { color:var(--accent); background:var(--accent-soft); font-weight:600; }
  .secondary { border-top:1px solid var(--line); padding-top:18px; }
  .bottom { margin-top:auto; padding:0 12px; } .bottom p { font-size:12px; line-height:1.7; color:var(--muted); } .bottom span { font-size:11px; }
  .new { border:1px solid var(--line-2); padding:9px 12px; border-radius:7px; background:var(--bg); color:var(--ink); }
  .activity { width:7px; height:7px; border-radius:50%; background:var(--accent); margin-left:auto; }
  .count { color:#923115; font-size:12px; margin-left:auto; }
  @media(max-width:1050px) { .app-navigation { padding:14px 20px; flex-direction:row; align-items:center; flex-wrap:wrap; gap:12px; border-right:0; border-bottom:1px solid var(--line); overflow:visible; } .brand { font-size:28px; padding:0; } .brand span,.bottom p { display:none; } nav { display:flex; flex-wrap:wrap; } nav button { padding:9px; } .secondary { border:0; padding:0; } .bottom { margin:0 0 0 auto; padding:0; } }
  @media(prefers-reduced-motion:reduce) { * { scroll-behavior:auto; } }
</style>
