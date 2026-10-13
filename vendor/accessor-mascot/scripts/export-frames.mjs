import { writeFileSync, mkdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { STATES, PALETTE, BearAnimator, renderPixels, renderScreen, stripAnsi } from '../src/bear.mjs';

const out = resolve(process.argv[2] || 'preview');
mkdirSync(out, {recursive:true});
const states = STATES.map(state => ({ ...state, frames: Array.from({length:24},(_,i)=>renderPixels(state.id,i*state.cycle/24)) }));
const animator = new BearAnimator('idle',0);
const tour = []; let clock = 0;
for (const state of STATES) {
  animator.setState(state.id, clock);
  for (let i=0;i<24;i++) {
    const now = clock+i*100;
    const sample = animator.sample(now);
    tour.push({state:state.id,pixels:renderPixels(sample.state,sample.time,sample.transition),screen:renderScreen(sample,{columns:80,rows:24,selectedState:state.id,color:false}).map(stripAnsi)});
  }
  clock+=2400;
}
writeFileSync(resolve(out,'frames.json'), JSON.stringify({palette:PALETTE,states,tour}));
console.log(`Exported ${states.length} states and ${tour.length} tour frames.`);
