export type SourceUnit={id:string;text:string;start:number|null};
export type DraftItem={kind:string;text:string;sourceId:string;quote:string;generatedText?:string;approved?:boolean;added?:boolean};
export type GroundedWork={sources:SourceUnit[];context:string;items:DraftItem[];discarded:number;template:string;createdAt:string};
export function hasSource(item:DraftItem, sources:SourceUnit[]) {
  return !!item.quote?.trim() && sources.some(s=>s.id===item.sourceId&&s.text.includes(item.quote));
}
