// Generate pixel hashes from the supplied renderer, independently of the Rust port.
// Run with Node only when changing the mascot: node scripts/export-mascot-reference.mjs
import { writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { STATES, PALETTE, BearAnimator, renderPixels } from '../vendor/accessor-mascot/src/bear.mjs';

const colors = [null, ...Object.keys(PALETTE)];
const hash = pixels => createHash('sha256').update(Buffer.from(pixels.flat().map(c => colors.indexOf(c)))).digest('hex');
const frames = [];
for (const state of STATES) {
  for (const phase of [0, .13, .33, .51, .73, .94]) {
    const time = state.cycle * phase;
    frames.push({ state: state.id, time, hash: hash(renderPixels(state.id, time)) });
  }
  for (const kind of ['blink', 'doze', 'wake', 'reassure']) {
    for (const progress of [.1, .4, .6, .9]) {
      const transition = { kind, progress };
      frames.push({ state: state.id, time: 137, transition, hash: hash(renderPixels(state.id, 137, transition)) });
    }
  }
}
const transitions = [];
for (const from of STATES) for (const to of STATES) {
  if (from.id === to.id) continue;
  const animator = new BearAnimator(from.id, 0);
  animator.setState(to.id, 613);
  const samples = [613, 773, 953, 1193, 1493].map(now => ({ now, hash: hash(animator.pixels(now)) }));
  // A second change interrupts the transition from whichever pose is displayed.
  const interrupted = new BearAnimator(from.id, 0);
  interrupted.setState(to.id, 613);
  interrupted.setState('happy', 713);
  samples.push({ now: 813, interrupted: true, hash: hash(interrupted.pixels(813)) });
  transitions.push({ from: from.id, to: to.id, samples });
}
writeFileSync(new URL('../tests/fixtures/mascot-reference.json', import.meta.url), JSON.stringify({ frames, transitions }) + '\n');
console.log(`Exported ${frames.length} frames and ${transitions.length} transitions.`);
