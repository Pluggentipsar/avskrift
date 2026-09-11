import {invoke} from '@tauri-apps/api/core';
import {get, writable} from 'svelte/store';

type Work = {id:string; command:string; cancelling:boolean; finished:boolean; error:string};
export const activeWork = writable<Work|null>(null);
export const isWorkCancelled = (message:string) => message.includes('Arbetet avbröts. Tidigare resultat finns kvar.');
let starting = false;

export async function invokeWork<T>(command:string, args:Record<string,unknown>):Promise<T> {
  if(starting || get(activeWork)) throw Error('Vänta tills det pågående arbetet är klart eller har avbrutits.');
  starting = true;
  let id:string|undefined;
  try {
    id = await invoke<string>('begin_work');
    if(!id) throw Error('Arbetet kunde inte startas. Försök igen.');
    activeWork.set({id,command,cancelling:false,finished:false,error:''});
    return await invoke<T>(command,{...args,workId:id});
  } finally {
    if(id) {
      // Workers own claimed registrations. This only removes a registration if dispatch failed.
      await invoke('forget_work',{id}).catch(()=>{});
      activeWork.update(current=>current?.id===id?null:current);
    }
    starting = false;
  }
}
export async function cancelActiveWork() {
  const work = get(activeWork);
  if(!work || work.cancelling || work.finished) return;
  const update = (patch:Partial<Work>) => activeWork.update(current=>current?.id===work.id?{...current,...patch}:current);
  update({cancelling:true,error:''});
  try {
    const accepted = await invoke<boolean>('cancel_work',{id:work.id});
    if(!accepted) update({cancelling:false,finished:true});
  } catch(e) { update({cancelling:false,error:`Kunde inte avbryta: ${String(e)}`}); }
  // Keep the UI occupied until the worker actually stops; cancellation is a request, not a kill.
}
