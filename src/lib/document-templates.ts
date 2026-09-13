import type {SourceUnit} from './grounded';
export type DocumentField={id:string;heading:string;instruction:string;missing:string};
export type DocumentTemplate={formatVersion:number;id:string;revision:number;name:string;purpose:string;fields:DocumentField[]};
export type FieldValue={id:string;text:string;status:string;evidence:{sourceId:string;quote:string}[];citationWarning?:boolean;originalText?:string;reviewed?:boolean;manual?:boolean};
export type TemplateDraft={id:string;createdAt:string;template:DocumentTemplate;sources:SourceUnit[];basis:string;sourceKind:'original'|'anonymized';origin:'local'|'external'|'manual'|'prepared';model?:string;values:FieldValue[];freeText?:string;reviewed?:boolean};
export type Handoff={template:DocumentTemplate;sources:SourceUnit[];basis:string;sourceKind:'original'|'anonymized';package:string};
export type TemplateWork={drafts:TemplateDraft[];activeId?:string;selected?:string;sourceKind?:'original'|'anonymized';editor?:DocumentTemplate|null;handoff?:Handoff|null;externalText?:string};
export const clone=<T>(value:T):T=>JSON.parse(JSON.stringify(value));
export function renderTemplateDraft(d:TemplateDraft):string {
  const reviewed=d.freeText!==undefined?!!d.reviewed:d.values.every(v=>v.reviewed);
  const header=`# ${d.template.name}\n\n${reviewed?'Granskat av användaren':'Ogranskat utkast'} · Mallversion ${d.template.revision}\nInget ärende har registrerats.\n`;
  return header+'\n'+(d.freeText!==undefined?d.freeText:d.template.fields.map(f=>`## ${f.heading}\n${d.values.find(v=>v.id===f.id)?.text||f.missing}`).join('\n\n'));
}
