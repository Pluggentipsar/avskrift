import { test } from 'node:test';
import assert from 'node:assert/strict';
import { transcriptBlocks, playbackIndex, matchingSegments, type Utterance } from '../src/lib/transcript-view.ts';
const segment = (start:number,end:number,text='Svensk text'):Utterance => ({start,end,text,speaker:null});

test('overlapping speaker tracks, gaps and exact end boundaries preserve first-match playback',()=>{
  const source=[segment(0,12),segment(2,4),segment(5,7),segment(14,18),segment(14,20)];
  const lookup=playbackIndex(source);
  for(let t=-1;t<22;t+=.25)assert.equal(lookup(t),source.findIndex(u=>u.start<=t&&t<u.end),String(t));
  const unsorted=[source[3],source[0],source[2]];
  for(let t=0;t<22;t++)assert.equal(playbackIndex(unsorted)(t),unsorted.findIndex(u=>u.start<=t&&t<u.end));
});
test('long single-speaker meetings are split without losing or duplicating segments',()=>{
  const source=Array.from({length:2000},(_,i)=>({...segment(i,i+1,'ord '.repeat(100)),speaker:'A'}));
  const blocks=transcriptBlocks(source);
  assert.ok(blocks.length>100);
  assert.equal(blocks[0].start,0);assert.equal(blocks.at(-1)?.end,source.length);
  for(let i=0;i<blocks.length;i++){
    assert.ok(blocks[i].end-blocks[i].start<=16);
    if(i)assert.equal(blocks[i-1].end,blocks[i].start);
  }
  assert.deepEqual(transcriptBlocks([]),[]);
});
test('search covers all text, Swedish case and phrases without interpreting regex characters',()=>{
  const texts=['Åsa i Örebro.','Vi ser [namn] i texten.','ÅSA I ÖREBRO igen.'].map(s=>s.toLocaleLowerCase('sv'));
  assert.deepEqual(matchingSegments(texts,' ÅSA I ÖREBRO '),[0,2]);
  assert.deepEqual(matchingSegments(texts,'[namn]'),[1]);
  assert.deepEqual(matchingSegments(texts,' '),[]);
  assert.deepEqual(matchingSegments(texts,'saknas'),[]);
});
