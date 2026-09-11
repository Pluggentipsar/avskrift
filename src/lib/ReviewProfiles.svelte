<script lang="ts">
  import {onMount} from 'svelte';
  type Settings={enabled:string[];terms:string[];useAi:boolean};
  type Profile={id:string;name:string;settings:Settings};
  let {current,onapply,disabled=false}: {current:Settings;onapply:(settings:Settings)=>void;disabled?:boolean}=$props();
  let profiles=$state<Profile[]>([]),name=$state(''),error=$state(''),selected=$state('');
  onMount(()=>{try{const stored=JSON.parse(localStorage.getItem('avskrift_review_profiles')||'[]');if(Array.isArray(stored))profiles=stored.filter(p=>typeof p.id==='string'&&typeof p.name==='string'&&Array.isArray(p.settings?.enabled)&&p.settings.enabled.every((k:unknown)=>typeof k==='string')&&Array.isArray(p.settings?.terms)&&p.settings.terms.every((k:unknown)=>typeof k==='string')).slice(0,30);}catch{error='Egna profiler kunde inte läsas.';}});
  function store(next:Profile[]){try{localStorage.setItem('avskrift_review_profiles',JSON.stringify(next));profiles=next;error='';return true;}catch{error='Profilen kunde inte sparas.';return false;}}
  function save(){const p={id:crypto.randomUUID(),name:name.trim(),settings:JSON.parse(JSON.stringify(current))};if(p.name&&store([...profiles,p])){selected=p.id;name='';}}
  function apply(){const p=profiles.find(p=>p.id===selected);if(p)onapply(JSON.parse(JSON.stringify(p.settings)));}
</script>
<details><summary>Egna granskningsprofiler</summary><div class="fields">
  <p>Kategorier, egen ordlista och valet av djupare granskning sparas på datorn.</p>
  {#if profiles.length}<label>Sparad profil<select bind:value={selected} {disabled}><option value="">Välj profil…</option>{#each profiles as p}<option value={p.id}>{p.name}</option>{/each}</select></label><button onclick={apply} disabled={disabled||!selected}>Använd och granska igen</button><button onclick={()=>{if(store(profiles.filter(p=>p.id!==selected)))selected='';}} disabled={disabled||!selected}>Ta bort profil</button>{/if}
  <label>Profilnamn<input bind:value={name} maxlength="80" {disabled} /></label><button onclick={save} disabled={disabled||!name.trim()||profiles.length>=30}>Spara aktuella val</button>
  {#if error}<p role="alert">{error}</p>{/if}
</div></details>
<style>
  details{margin:16px 0;font:13px/1.6 Archivo,sans-serif}summary{color:var(--accent);cursor:pointer}.fields,label{display:grid;gap:8px}.fields{margin-top:12px}p{color:var(--muted);margin:0}input,select,button{box-sizing:border-box;width:100%;font:inherit;border:1px solid var(--line-2);border-radius:5px;padding:8px;color:var(--ink);background:var(--bg)}button{cursor:pointer}button:disabled{opacity:.5}:is(input,select,button,summary):focus-visible{outline:2px solid var(--accent);outline-offset:3px}
</style>
