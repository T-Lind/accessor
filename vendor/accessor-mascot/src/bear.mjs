import readline from 'node:readline';

export const STATES = Object.freeze([
  { id: 'idle', name: 'Idle', key: '1', message: 'Right here when you need me.', motion: 'Breathing & a gentle blink', cycle: 4000 },
  { id: 'greeting', name: 'Greeting', key: '2', message: 'Hello, friend. Shall we begin?', motion: 'A friendly little wave', cycle: 1600 },
  { id: 'curious', name: 'Curious', key: '3', message: 'Ooh. What have we found?', motion: 'Head tilt & a raised paw', cycle: 2400 },
  { id: 'thinking', name: 'Thinking', key: '4', message: 'Let me give that a little thought.', motion: 'Pondering & drifting dots', cycle: 2400 },
  { id: 'working', name: 'Working', key: '5', message: 'Paws on the keys. On it.', motion: 'Alternating typing paws', cycle: 1200 },
  { id: 'focused', name: 'Focused', key: '6', message: 'One thing at a time. We have this.', motion: 'Glasses & a measured nod', cycle: 2800 },
  { id: 'happy', name: 'Happy', key: '7', message: 'Well, this is rather lovely.', motion: 'Happy eyes & a soft sway', cycle: 1800 },
  { id: 'success', name: 'Success', key: '8', message: 'All done. Nicely done, us.', motion: 'Paw up & a proud nod', cycle: 2000 },
  { id: 'surprised', name: 'Surprised', key: '9', message: 'Oh! I did not see that coming.', motion: 'Wide eyes & a tiny hop', cycle: 1600 },
  { id: 'confused', name: 'Confused', key: 'a', message: 'Hmm. Let us look at that again.', motion: 'Uneven eyes & a head tilt', cycle: 2200 },
  { id: 'worried', name: 'Worried', key: 'b', message: 'A small snag. I am still with you.', motion: 'Tucked paws & worried brows', cycle: 2200 },
  { id: 'error', name: 'Error', key: 'c', message: 'That did not work. We can try again.', motion: 'A gentle shake, never a scold', cycle: 2000 },
  { id: 'sleepy', name: 'Sleepy', key: 'd', message: 'Just resting these little paws.', motion: 'Heavy eyelids & a yawn', cycle: 3200 },
  { id: 'sleeping', name: 'Sleeping', key: 'e', message: 'Quietly dreaming of honey.', motion: 'Slow breathing & floating zzz', cycle: 3600 },
  { id: 'celebrating', name: 'Celebrating', key: 'f', message: 'You did it! A little bear cheer.', motion: 'Dancing paws & confetti', cycle: 1800 }
].map(Object.freeze));

export const PALETTE = Object.freeze({
  outline: '#704a38', fur: '#d89863', highlight: '#f7c99a', shade: '#b36e48',
  muzzle: '#ffe3bc', ink: '#291e21', blush: '#ed9b89', mint: '#7fe3bf',
  teal: '#399e88', cream: '#edf0dc', rose: '#f5a6a0', gold: '#efcb76',
  background: '#101718', surface: '#172122', border: '#344546', muted: '#94a8a6'
});
export const PIXEL_WIDTH = 40;
export const PIXEL_HEIGHT = 36;
const LOOKUP = new Map(STATES.map(state => [state.id, state]));
const TAU = Math.PI * 2;
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
const lerp = (a, b, t) => a + (b - a) * t;
export function getState(id) {
  const state = LOOKUP.get(id);
  if (!state) throw new RangeError(`Unknown state "${id}". Choose: ${STATES.map(s => s.id).join(', ')}.`);
  return state;
}

class Canvas {
  constructor() { this.pixels = Array.from({ length: PIXEL_HEIGHT }, () => Array(PIXEL_WIDTH).fill(null)); }
  dot(x, y, color) {
    x = Math.round(x); y = Math.round(y);
    if (x >= 0 && x < PIXEL_WIDTH && y >= 0 && y < PIXEL_HEIGHT) this.pixels[y][x] = color;
  }
  rect(x, y, w, h, color) { for (let j = 0; j < h; j++) for (let i = 0; i < w; i++) this.dot(x + i, y + j, color); }
  rows(x, y, rows, color) {
    rows.forEach((row, j) => { for (let i = row[0]; i <= row[1]; i++) this.dot(x + i, y + j, color); });
  }
  disk(x, y, rx, ry, color) {
    for (let j = -ry; j <= ry; j++) for (let i = -rx; i <= rx; i++) {
      if ((i / rx) ** 2 + (j / ry) ** 2 <= 1.15) this.dot(x + i, y + j, color);
    }
  }
  stamp(x, y, pattern, color) {
    pattern.forEach((row, j) => [...row].forEach((ch, i) => { if (ch !== ' ') this.dot(x + i, y + j, color); }));
  }
}
const GLYPHS = {
  '?': ['###', '  #', ' ##', '   ', ' # '], '!': [' # ', ' # ', ' # ', '   ', ' # '],
  z: ['###', '  #', ' # ', '#  ', '###'],
  check: ['    #', '   # ', '# #  ', ' #   '],
  heart: [' # # ', '#####', '#####', ' ### ', '  #  '],
  star: ['  #  ', ' ### ', '#####', ' ### ', '  #  ']
};

function poseFor(state, time, transition) {
  const p = (Math.max(0, time) % state.cycle) / state.cycle;
  const wave = Math.sin(p * TAU);
  const pose = { x: 0, y: 0, headX: 0, headY: 0, blink: p > .92 && p < .97, eyes: 'open', mouth: 'smile', left: 'rest', right: 'rest', prop: null, bounce: 0 };
  switch (state.id) {
    case 'idle': pose.y = p < .5 ? 0 : -1; break;
    case 'greeting': pose.right = wave > 0 ? 'wave-high' : 'wave-low'; pose.eyes = p > .8 ? 'happy' : 'open'; break;
    case 'curious': pose.headX = wave > .25 ? 1 : -1; pose.headY = p < .4 ? -1 : 0; pose.left = 'chin'; pose.prop = '?'; break;
    case 'thinking': pose.left = 'chin'; pose.eyes = 'look'; pose.headY = p > .5 ? 1 : 0; pose.prop = 'dots'; break;
    case 'working': pose.prop = 'laptop'; pose.left = p % .25 < .125 ? 'type-up' : 'type-down'; pose.right = p % .25 < .125 ? 'type-down' : 'type-up'; pose.eyes = 'look-down'; pose.headY = p % .5 < .25 ? 0 : 1; break;
    case 'focused': pose.prop = 'glasses'; pose.eyes = 'look-down'; pose.headY = wave > .7 ? 1 : 0; pose.mouth = 'small'; break;
    case 'happy': pose.eyes = 'happy'; pose.x = wave > .3 ? 1 : wave < -.3 ? -1 : 0; pose.prop = 'heart'; break;
    case 'success': pose.right = p < .5 ? 'wave-high' : 'wave-low'; pose.eyes = 'happy'; pose.headY = p > .55 && p < .8 ? 1 : 0; pose.prop = 'check'; break;
    case 'surprised': pose.eyes = 'wide'; pose.mouth = 'o'; pose.y = p < .2 ? -2 : p < .35 ? -1 : 0; pose.left = 'out'; pose.right = 'out'; pose.prop = '!'; break;
    case 'confused': pose.headX = wave > 0 ? 1 : -1; pose.eyes = 'uneven'; pose.mouth = 'slant'; pose.left = 'chin'; pose.prop = '?'; break;
    case 'worried': pose.eyes = 'worried'; pose.mouth = 'frown'; pose.left = 'tuck'; pose.right = 'tuck'; pose.y = p < .5 ? 0 : 1; break;
    case 'error': pose.headX = p < .3 ? Math.round(Math.sin(p / .3 * TAU * 2)) : 0; pose.eyes = 'sad'; pose.mouth = 'frown'; pose.prop = 'error'; break;
    case 'sleepy': pose.eyes = 'sleepy'; pose.headY = p > .4 ? 1 : 0; pose.mouth = p > .3 && p < .7 ? 'o' : 'small'; pose.right = p > .3 && p < .7 ? 'chin' : 'rest'; break;
    case 'sleeping': pose.eyes = 'closed'; pose.headY = 1; pose.y = p < .5 ? 0 : 1; pose.mouth = 'small'; pose.prop = 'zzz'; break;
    case 'celebrating': pose.eyes = 'happy'; pose.mouth = 'grin'; pose.left = p < .5 ? 'wave-high' : 'wave-low'; pose.right = p < .5 ? 'wave-low' : 'wave-high'; pose.y = Math.abs(wave) > .6 ? -2 : 0; pose.x = wave > .3 ? 1 : wave < -.3 ? -1 : 0; pose.prop = 'confetti'; break;
  }
  if (transition) {
    const t = transition.progress;
    const envelope = Math.sin(t * Math.PI);
    pose.blink = t > .24 && t < .7;
    if (transition.kind === 'doze') { pose.headY += Math.round(envelope); pose.y += Math.round(envelope); }
    else if (transition.kind === 'wake') { pose.headY -= Math.round(envelope * 2); pose.right = t > .5 ? 'wave-low' : 'rest'; }
    else if (transition.kind === 'reassure') { pose.headX += Math.round(Math.sin(t * TAU) * envelope); }
    else { pose.y += t < .5 ? Math.round(envelope) : -Math.round(envelope * 2); }
  }
  return { ...pose, phase: p };
}

/** One semantic colour per pixel; null is transparent. No terminal side effects. */
export function renderPixels(id = 'idle', time = 0, transition = null) {
  const state = getState(id);
  const p = poseFor(state, time, transition);
  const c = new Canvas();
  const x = 19 + p.x, y = 20 + p.y;
  // Feet and silhouette: rounded outlines remain consistent across expressions.
  c.rect(x - 7, y + 10, 6, 3, 'outline'); c.rect(x + 2, y + 10, 6, 3, 'outline');
  c.rect(x - 6, y + 10, 4, 2, 'fur'); c.rect(x + 3, y + 10, 4, 2, 'fur');
  c.disk(x, y + 6, 8, 6, 'outline'); c.disk(x, y + 6, 7, 5, 'fur');
  c.disk(x - 1, y + 6, 4, 4, 'highlight');
  // Resting arms sit behind the scarf; raised arms stay clear of the face.
  const paw = (side, mode) => {
    const s = side === 'left' ? -1 : 1;
    let px = x + s * 9, py = y + 5, rx = 2, ry = 4;
    if (mode === 'wave-high') { px = x + s * 13; py = y - 7; ry = 3; c.rect(x + s * 10 - 1, y - 4, 3, 8, 'outline'); c.rect(x + s * 10, y - 3, 1, 7, 'fur'); }
    if (mode === 'wave-low') { px = x + s * 12; py = y - 3; ry = 3; c.rect(x + s * 10 - 1, y - 2, 3, 6, 'outline'); }
    if (mode === 'out') { px = x + s * 11; py = y + 1; ry = 2; }
    if (mode === 'chin') { px = x + s * 5; py = y - 1; ry = 2; }
    if (mode === 'tuck') { px = x + s * 3; py = y + 5; ry = 2; }
    if (mode.startsWith('type')) { px = x + s * 5; py = y + (mode === 'type-up' ? 5 : 6); ry = 1; }
    c.disk(px, py, rx, ry, 'outline'); c.disk(px, py - .5, rx - 1, ry - 1 || 1, 'fur');
    if (mode.startsWith('wave')) { c.dot(px, py - 1, 'muzzle'); c.dot(px - 1, py, 'blush'); c.dot(px + 1, py, 'blush'); }
  };
  if (!['chin', 'tuck'].includes(p.left)) paw('left', p.left);
  if (!['chin', 'tuck'].includes(p.right)) paw('right', p.right);
  c.rect(x - 7, y, 15, 3, 'teal'); c.rect(x - 7, y, 15, 2, 'mint');
  c.rect(x + 4, y + 2, 3, 6, 'teal'); c.rect(x + 4, y + 2, 2, 5, 'mint'); c.dot(x + 5, y + 7, 'mint');
  const hx = x + p.headX, hy = 11 + p.y + p.headY;
  // Ears: warm centres, little highlights, and a softly squared pixel silhouette.
  c.disk(hx - 7, hy - 5, 4, 4, 'outline'); c.disk(hx + 7, hy - 5, 4, 4, 'outline');
  c.disk(hx - 7, hy - 5, 3, 3, 'fur'); c.disk(hx + 7, hy - 5, 3, 3, 'fur');
  c.disk(hx - 7, hy - 5, 1, 2, 'blush'); c.disk(hx + 7, hy - 5, 1, 2, 'blush');
  const head = [[-6,6],[-8,8],[-9,9],[-10,10],[-10,10],[-10,10],[-10,10],[-10,10],[-9,9],[-9,9],[-8,8],[-6,6]];
  c.rows(hx, hy - 4, head, 'outline');
  c.rows(hx, hy - 3, head.slice(1,-1).map(([a,b]) => [a+1,b-1]), 'fur');
  c.rect(hx - 6, hy - 3, 10, 1, 'highlight'); c.rect(hx - 8, hy - 2, 2, 3, 'highlight');
  c.rows(hx, hy + 5, [[-7,7],[-5,5]], 'shade');
  c.disk(hx, hy + 4, 5, 3, 'muzzle');
  c.rect(hx - 8, hy + 3, 2, 1, 'blush'); c.rect(hx + 7, hy + 3, 2, 1, 'blush');
  const eye = (ex, mode, side) => {
    const ey = hy + 1;
    if (p.blink || mode === 'closed') { c.rect(ex - 1, ey + 1, 3, 1, 'ink'); return; }
    if (mode === 'happy') { c.stamp(ex - 1, ey, [' # ', '# #'], 'ink'); return; }
    if (mode === 'sleepy') { c.rect(ex - 1, ey + 1, 3, 1, 'ink'); c.dot(ex + 1, ey + 2, 'ink'); return; }
    if (mode === 'wide') { c.rect(ex - 1, ey - 1, 3, 4, 'muzzle'); c.rect(ex, ey, 2, 3, 'ink'); c.dot(ex, ey, 'cream'); return; }
    if (mode === 'uneven' && side === 1) { c.rect(ex - 1, ey, 3, 1, 'ink'); c.dot(ex + 1, ey + 1, 'ink'); return; }
    c.rect(ex, ey + (mode === 'look-down' ? 1 : 0), 2, 2, 'ink');
    c.dot(ex + (mode === 'look' ? 1 : 0), ey + (mode === 'look-down' ? 1 : 0), 'cream');
    if (mode === 'worried' || mode === 'sad') { c.dot(ex + (side < 0 ? 1 : 0), ey - 2, 'ink'); c.dot(ex + (side < 0 ? 0 : 1), ey - 1, 'ink'); }
  };
  eye(hx - 5, p.eyes, -1); eye(hx + 4, p.eyes, 1);
  c.rect(hx - 1, hy + 3, 3, 1, 'ink'); c.dot(hx, hy + 4, 'ink');
  if (p.mouth === 'o') c.stamp(hx - 1, hy + 5, ['###','# #','###'], 'ink');
  else if (p.mouth === 'frown') c.stamp(hx - 2, hy + 6, [' ### ', '#   #'], 'ink');
  else if (p.mouth === 'slant') c.stamp(hx - 2, hy + 6, ['##   ', '  ###'], 'ink');
  else if (p.mouth === 'grin') c.stamp(hx - 3, hy + 5, ['#     #', ' ##### ', '  ###  '], 'ink');
  else if (p.mouth === 'small') c.rect(hx - 1, hy + 6, 3, 1, 'ink');
  else c.stamp(hx - 2, hy + 5, ['# # #', ' ### '], 'ink');
  if (p.prop === 'glasses') {
    c.stamp(hx - 7, hy - 1, ['##### #####', '#   ###   #', '#   # #   #', '##### #####'], 'teal');
    c.rect(hx - 9, hy, 2, 1, 'teal'); c.rect(hx + 6, hy, 3, 1, 'teal');
  }
  if (['chin','tuck'].includes(p.left)) paw('left', p.left);
  if (['chin','tuck'].includes(p.right)) paw('right', p.right);
  if (p.prop === 'laptop') {
    c.rect(x - 7, y + 5, 15, 6, 'outline'); c.rect(x - 6, y + 5, 13, 5, 'teal'); c.rect(x - 5, y + 6, 11, 3, 'surface');
    c.stamp(x - 3, y + 6, ['#   ', ' #  ', '# ##'], 'mint'); c.rect(x - 9, y + 11, 19, 1, 'mint');
    c.dot(x - 5, y + (p.left === 'type-up' ? 4 : 5), 'muzzle'); c.dot(x + 5, y + (p.right === 'type-up' ? 4 : 5), 'muzzle');
  }
  if (p.prop === '?' || p.prop === '!') c.stamp(33, 4 + (p.phase < .5 ? 0 : -1), GLYPHS[p.prop], p.prop === '!' ? 'gold' : 'mint');
  if (p.prop === 'check') c.stamp(31, 4 + (p.phase < .5 ? 0 : 1), GLYPHS.check, 'mint');
  if (p.prop === 'heart') c.stamp(31, 5 + Math.round(Math.sin(p.phase * TAU)), GLYPHS.heart, 'rose');
  if (p.prop === 'dots') {
    c.rect(30, 3, 8, 5, 'border'); c.rect(31, 3, 6, 4, 'surface'); c.dot(30, 8, 'border');
    const n = 1 + Math.floor(p.phase * 3); for (let i = 0; i < n; i++) c.dot(32 + i * 2, 5, 'mint');
  }
  if (p.prop === 'error') c.stamp(33, 5, ['# #',' # ','# #'], 'rose');
  if (p.prop === 'zzz') {
    const dy = Math.floor(p.phase * 3); c.stamp(30, 8 - dy, GLYPHS.z, 'mint');
    if (p.phase > .28) c.stamp(34, 4 - dy, GLYPHS.z, 'muted');
  }
  if (p.prop === 'confetti') {
    for (let i = 0; i < 12; i++) {
      const cx = i % 2 === 0 ? 2 + (i * 7 % 7) : 31 + (i * 3 % 7);
      const cy = Math.floor((p.phase * 23 + i * 5) % 25);
      c.rect(cx, cy, i % 3 === 0 ? 2 : 1, i % 3 === 0 ? 1 : 2, ['mint','rose','gold'][i % 3]);
    }
  }
  return c.pixels;
}

/** Stateful timing only; callers own the clock and rendering surface. */
export class BearAnimator {
  constructor(state = 'idle', now = 0, { reducedMotion = false } = {}) {
    getState(state); this.state = state; this.enteredAt = now; this.transition = null; this.reducedMotion = reducedMotion;
  }
  setState(next, now = performance.now()) {
    getState(next);
    if (next === this.state) return false;
    const previous = this.sample(now);
    const from = previous.state;
    this.state = next; this.enteredAt = now;
    const kind = next === 'sleeping' || next === 'sleepy' ? 'doze' : ['sleeping','sleepy'].includes(from) ? 'wake' : ['worried','error'].includes(next) ? 'reassure' : 'blink';
    this.transition = this.reducedMotion ? null : { from, to: next, kind, startedAt: now, duration: kind === 'wake' ? 680 : kind === 'doze' ? 760 : 480, fromTime: previous.time };
    return true;
  }
  sample(now = performance.now()) {
    if (this.reducedMotion) return { state: this.state, time: 0, transition: null };
    const tr = this.transition;
    if (tr && now - tr.startedAt < tr.duration) {
      const progress = clamp((now - tr.startedAt) / tr.duration, 0, 1);
      return { state: progress < .5 ? tr.from : tr.to, time: progress < .5 ? tr.fromTime + now - tr.startedAt : now - tr.startedAt - tr.duration / 2, transition: { ...tr, progress } };
    }
    if (tr) this.enteredAt = tr.startedAt + tr.duration / 2;
    this.transition = null;
    return { state: this.state, time: Math.max(0, now - this.enteredAt), transition: null };
  }
  pixels(now = performance.now()) { const f = this.sample(now); return renderPixels(f.state, f.time, f.transition); }
}

const rgb = hex => [1,3,5].map(i => parseInt(hex.slice(i,i+2),16));
const COLOR_CACHE = new Map();
function ansiColor(name, background = false, mode = 'truecolor') {
  if (!name) return background ? '\x1b[49m' : '\x1b[39m';
  const key = `${name}:${background}:${mode}`;
  if (!COLOR_CACHE.has(key)) {
    const [r,g,b] = rgb(PALETTE[name] || name);
    const value = mode === '256' ? `5;${16 + 36 * Math.round(r/255*5) + 6 * Math.round(g/255*5) + Math.round(b/255*5)}` : `2;${r};${g};${b}`;
    COLOR_CACHE.set(key, `\x1b[${background ? 48 : 38};${value}m`);
  }
  return COLOR_CACHE.get(key);
}
export function renderArt(pixels, { color = 'truecolor' } = {}) {
  const lines = [];
  for (let y = 0; y < pixels.length; y += 2) {
    let row = '', fg = undefined, bg = undefined;
    for (let x = 0; x < pixels[y].length; x++) {
      const top = pixels[y][x], bottom = pixels[y+1]?.[x] || null;
      const nextFg = top || bottom, nextBg = top && bottom ? bottom : null;
      if (fg !== nextFg) { row += ansiColor(nextFg, false, color); fg = nextFg; }
      if (bg !== nextBg) { row += ansiColor(nextBg, true, color); bg = nextBg; }
      row += top ? '▀' : bottom ? '▄' : ' ';
    }
    lines.push(row + '\x1b[0m');
  }
  return lines;
}

/** Portable character-art fallback for monochrome and non-Unicode terminals. */
export function renderAscii(id = 'idle', time = 0, transition = null) {
  const p = poseFor(getState(id), time, transition);
  const eyes = p.blink || ['closed','sleepy'].includes(p.eyes) ? '-   -' : p.eyes === 'happy' ? '^   ^' : p.eyes === 'wide' ? 'O   O' : p.eyes === 'uneven' ? 'o   -' : 'o   o';
  const mouth = p.mouth === 'o' ? ' O ' : p.mouth === 'frown' ? ' ^ ' : p.mouth === 'slant' ? ' ~ ' : 'w';
  const shift = ' '.repeat(8 + p.x + p.headX);
  const right = p.right.startsWith('wave') ? '  o/' : '';
  let rows = [
    shift+' .--.     .--.', shift+'(    )___(    )', shift+' /           \\',
    shift+`|   ${eyes}   |${right}`, shift+'|     ._.     |', shift+` \\    ${mouth.padStart(2).padEnd(3)}    /`,
    shift+'  \'--------\'', shift+'  /===[A]===\\', shift+' /  .-----\\  \\',
    shift+'(  /       \\  )', shift+' \\|       | /', shift+'  (___) (___)'
  ];
  if (p.prop === 'laptop') rows.splice(8, 3, shift+' / .--------. \\', shift+'(  |  >_    |  )', shift+' __|________|__');
  const prop = p.prop === 'zzz' ? ' z z z' : p.prop === '?' ? ' ?' : p.prop === '!' ? ' !' : p.prop === 'heart' ? ' <3' : p.prop === 'check' ? ' OK' : p.prop === 'error' ? ' x' : p.prop === 'dots' ? ' ...' : '';
  rows[1] += prop;
  if (p.prop === 'confetti') { rows[0] = '  *   + '+rows[0]+'  +'; rows[3] += ' *'; }
  return [''.padEnd(40), ...rows.map(s => s.padEnd(40).slice(0,40)), ...Array(5).fill(''.padEnd(40))];
}

const textColor = (str, name, color) => color ? ansiColor(name, false, color) + str + '\x1b[0m' : str;
export const stripAnsi = str => str.replace(/\x1b\[[0-9;?]*[a-zA-Z]/g, '');
const fit = (s, width) => s.slice(0, width).padEnd(width);
export function renderScreen(sample, { columns = 80, rows = 24, ascii = false, color = 'truecolor', selectedState = sample.transition?.to || sample.state, paused = false, demo = false } = {}) {
  const state = getState(selectedState);
  const width = Math.max(1, Math.min(78, columns - 1));
  const ui = (s,name='cream') => textColor(s, name, color);
  const art = ascii || !color ? renderAscii(sample.state, sample.time, sample.transition) : renderArt(renderPixels(sample.state,sample.time,sample.transition),{color});
  if (columns < 44 || rows < 16) {
    return [ui(fit('accessor / '+state.name,width),'mint'), ui(fit(state.message,width)), ui(fit('Enlarge to 44 x 16 for the bear.',width),'muted'), ui(fit('q quit | 1-9 a-f',width),'muted')].slice(0,Math.max(1,rows-1));
  }
  const wide = width >= 73 && rows >= 24;
  const short = rows < 24;
  const leftWidth = 42;
  const result = [
    ui(fit('  accessor',width),'mint'),
    ui(fit('  A LITTLE BEAR. A LITTLE COMPANY.',width),'muted'),
    ui((ascii ? '-' : '─').repeat(width),'border')
  ];
  // Short windows use the same character design at a compact size.
  const compactArt = short && !ascii && color ? renderArt(renderPixels(sample.state,sample.time,sample.transition).filter((_,i)=>i>=2&&i<34).map(row=>row.filter((_,i)=>i>=1&&i<39)),{color}) : art;
  const bodyBudget = Math.max(0, rows - 1 - 3 - (rows >= 20 ? 3 : 2));
  let displayedArt = compactArt.slice(0, bodyBudget);
  if (short && rows < 22) {
    const fallback = renderAscii(sample.state,sample.time,sample.transition).slice(1,13);
    // Remove redundant neck and torso rows in very short windows, keeping feet.
    while (fallback.length > bodyBudget) fallback.splice(fallback.length > 10 ? 7 : 8, 1);
    displayedArt = fallback;
  }
  for (let i=0;i<displayedArt.length;i++) {
    if (wide) {
      let right = '';
      if(i===0) right = ui('  THE MOOD BOARD','muted');
      if(i>=2&&i<17) {
        const s = STATES[i-2]; const active = s.id===selectedState;
        right = ui(`  ${active ? (ascii ? '>' : '›') : ' '} ${s.key}  ${s.name.padEnd(13)}${active ? (ascii ? '*' : '●') : ' '}`,active?'mint':'muted');
      }
      result.push(' '+displayedArt[i]+' '.repeat(Math.max(0,leftWidth-1-stripAnsi(displayedArt[i]).length))+right);
    } else {
      const indent = Math.max(0,Math.floor((width-stripAnsi(displayedArt[i]).length)/2));
      result.push(' '.repeat(indent)+displayedArt[i]);
    }
  }
  result.push(ui(fit(`  ${ascii ? '*' : '●'} ${state.name.toLowerCase()}  ${paused ? '/ paused' : demo ? '/ on tour' : '/ ready'}`,width),'mint'));
  if(rows>=20) result.push(ui(fit('  '+state.message,width),'cream'));
  const controls = width < 65 ? ' q quit | 1–9 a–f | ←→ | space | t tour' : '  1–9 / a–f moods   ← → browse   space pause   t tour   q quit';
  result.push(ui(fit(controls.replaceAll(ascii ? '–' : '\0','-').replaceAll(ascii ? '← →' : '\0','< >').replaceAll(ascii ? '←→' : '\0','<>'),width),'muted'));
  return result.slice(0,Math.max(1,rows-1));
}

/** Interactive terminal host. createMascot is also usable by an existing CLI. */
export function createMascot({ input = process.stdin, output = process.stdout, state = 'idle', demo = false, reducedMotion = false, ascii = false, color, fps = 12, duration = 0 } = {}) {
  getState(state);
  if (!Number.isFinite(fps) || fps < 1 || fps > 60) throw new RangeError('fps must be between 1 and 60.');
  color = color === undefined ? (process.env.NO_COLOR !== undefined || process.env.TERM === 'dumb' ? false : process.env.COLORTERM?.includes('color') || process.env.TERM?.includes('direct') ? 'truecolor' : '256') : color;
  const clock = () => performance.now();
  const animator = new BearAnimator(state, clock(), { reducedMotion });
  let started = false, closed = false, interval = null, paused = false, pausedAt = 0, pausedTotal = 0, lastDemo = 0, finishTimer, originalRaw = false;
  let resolveDone;
  const done = new Promise(resolve => { resolveDone = resolve; });
  const now = () => (paused ? pausedAt : clock()) - pausedTotal;
  const change = id => { animator.setState(id, now()); lastDemo = now(); };
  const draw = () => {
    if (closed) return;
    const t = now();
    if (demo && !paused && t-lastDemo >= 2700) {
      change(STATES[(STATES.findIndex(s=>s.id===animator.state)+1)%STATES.length].id);
    }
    const lines = renderScreen(animator.sample(t), {columns:output.columns||80,rows:output.rows||24,ascii,color,selectedState:animator.state,paused,demo});
    output.write('\x1b[H'+lines.map(line=>line+'\x1b[K').join('\r\n')+'\x1b[J');
  };
  const onKey = (str, key = {}) => {
    if (key.name === 'q' || key.name === 'escape' || key.ctrl && key.name === 'c') return stop();
    if (key.name === 'space') {
      if (paused) { pausedTotal += clock()-pausedAt; paused = false; }
      else { pausedAt=clock(); paused=true; }
    } else if (key.name === 't') { demo=!demo; lastDemo=now(); }
    else if (['left','right','up','down'].includes(key.name)) {
      demo=false; const delta=['left','up'].includes(key.name)?-1:1;
      change(STATES[(STATES.findIndex(s=>s.id===animator.state)+delta+STATES.length)%STATES.length].id);
    } else {
      const next=STATES.find(s=>s.key===str?.toLowerCase()); if(next) {demo=false;change(next.id);}
    }
    draw();
  };
  const onSignal = () => stop();
  const onOutputError = error => { stop(); if (error.code !== 'EPIPE') process.exitCode = 1; };
  function stop() {
    if (closed) return;
    closed=true;
    clearInterval(interval); clearTimeout(finishTimer);
    if (started && output.isTTY) {
      input.off('keypress',onKey); output.off('resize',draw);
      for (const signal of ['SIGINT','SIGTERM','SIGHUP']) process.off(signal,onSignal);
      process.off('exit',stop);
      if(input.isTTY && input.setRawMode) input.setRawMode(originalRaw);
      if(input.isTTY && !originalRaw) input.pause();
      if(!output.destroyed) output.write('\x1b[0m\x1b[?25h\x1b[?1049l');
    }
    output.off('error',onOutputError);
    resolveDone();
  }
  return {
    animator, done,
    setState(id) { change(id); if(started&&!closed&&output.isTTY)draw(); return this; },
    start() {
      if(started||closed)return this;
      started=true; lastDemo=now();
      if(!output.isTTY) {
        // Pipes get one legible snapshot, with no cursor or colour escapes.
        output.write(renderAscii(animator.state,0).map(line=>line.trimEnd()).join('\n')+'\n'+getState(animator.state).name+': '+getState(animator.state).message+'\n');
        stop();return this;
      }
      output.on('error',onOutputError);
      output.write('\x1b[?1049h\x1b[2J\x1b[H\x1b[?25l');
      if(input.isTTY) {originalRaw=!!input.isRaw;readline.emitKeypressEvents(input);input.setRawMode?.(true);input.resume();input.on('keypress',onKey);}
      output.on('resize',draw);
      for(const signal of ['SIGINT','SIGTERM','SIGHUP'])process.on(signal,onSignal);
      process.on('exit',stop);
      draw(); interval=setInterval(draw,1000/fps);
      if(duration>0)finishTimer=setTimeout(stop,duration*1000);
      return this;
    },
    stop
  };
}
