import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { EventEmitter } from 'node:events';
import { STATES, PALETTE, BearAnimator, renderPixels, renderScreen, stripAnsi, createMascot } from '../src/bear.mjs';

test('fifteen distinct states animate within the fixed canvas and palette', () => {
  assert.equal(STATES.length,15);
  const signatures = new Set();
  for(const state of STATES) {
    const poses=[];
    for(let i=0;i<24;i++) {
      const pixels=renderPixels(state.id,i*state.cycle/24);
      assert.equal(pixels.length,36); assert.ok(pixels.every(r=>r.length===40));
      for(const row of pixels) for(const pixel of row) assert.ok(pixel===null || Object.hasOwn(PALETTE,pixel));
      poses.push(JSON.stringify(pixels));
    }
    assert.ok(new Set(poses).size>1,`${state.id} must visibly animate`);
    signatures.add(poses.join(''));
  }
  assert.equal(signatures.size,15);
});

test('all 210 directed mood changes settle into the requested state',()=>{
  for(const from of STATES) for(const to of STATES) {
    if(from.id===to.id)continue;
    const animator=new BearAnimator(from.id,0);
    animator.setState(to.id,100);
    assert.equal(animator.sample(100).state,from.id);
    const mid=animator.sample(400);
    assert.ok(mid.transition);
    const final=animator.sample(1000);
    assert.equal(final.state,to.id);assert.equal(final.transition,null);
    const next=animator.sample(1010);
    assert.equal(next.time-final.time,10,'loop clock must be continuous after settling');
  }
});

test('rapid interruptions remain valid and reduced motion stays static',()=>{
  const animator=new BearAnimator('idle',0);
  for(let i=0;i<60;i++){animator.setState(STATES[i%15].id,i*40);assert.ok(animator.pixels(i*40+20).length===36);}
  assert.equal(animator.sample(4000).state,'celebrating');
  const reduced=new BearAnimator('idle',0,{reducedMotion:true});
  reduced.setState('sleeping',50);
  assert.deepEqual(reduced.sample(60),reduced.sample(5000));
  assert.equal(reduced.sample(60).state,'sleeping');
  assert.throws(()=>reduced.setState('angry',50),RangeError);
});

test('terminal layout fits and keeps controls visible at supported sizes',()=>{
  for(const [columns,rows] of [[80,24],[100,35],[60,24],[44,20],[50,18],[44,16],[20,8]]) {
    for(const ascii of [true,false]) {
      const lines=renderScreen({state:'celebrating',time:500,transition:null},{columns,rows,ascii});
      assert.ok(lines.length<rows);
      assert.ok(lines.every(s=>stripAnsi(s).length<columns),`${columns}x${rows} must fit`);
      assert.ok(stripAnsi(lines.at(-1)).includes('quit'));
    }
  }
});

test('pipes get one readable snapshot with no ANSI control sequences',()=>{
  const result=spawnSync(process.execPath,['bin/accessor-mascot.mjs','--state','working','--demo'],{cwd:new URL('..',import.meta.url),encoding:'utf8'});
  assert.equal(result.status,0);assert.ok(result.stdout.includes('Paws on the keys'));
  assert.ok(!result.stdout.includes('\x1b'));
  const bad=spawnSync(process.execPath,['bin/accessor-mascot.mjs','--state','unknown'],{cwd:new URL('..',import.meta.url),encoding:'utf8'});
  assert.equal(bad.status,1);assert.ok(bad.stderr.includes('Unknown state'));
});

test('terminal session restores cursor, screen, raw mode and listeners',async()=>{
  const input=new EventEmitter();input.isTTY=true;input.isRaw=false;
  input.setRawMode=value=>{input.isRaw=value;};input.resume=()=>{};input.pause=()=>{};
  const output=new EventEmitter();output.isTTY=true;output.columns=80;output.rows=24;let written='';output.write=s=>{written+=s;};
  const listeners=process.listenerCount('SIGINT');
  const bear=createMascot({input,output,color:false}).start();
  assert.equal(input.isRaw,true);
  input.emit('keypress','5',{name:'5'});assert.equal(bear.animator.state,'working');
  input.emit('keypress','',{name:'right'});assert.equal(bear.animator.state,'focused');
  input.emit('keypress','q',{name:'q'});
  await bear.done;
  assert.equal(input.isRaw,false);assert.equal(process.listenerCount('SIGINT'),listeners);
  assert.ok(written.includes('\x1b[?25l'));assert.ok(written.endsWith('\x1b[0m\x1b[?25h\x1b[?1049l'));
  assert.equal(input.listenerCount('keypress'),0);
});
