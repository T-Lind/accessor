#!/usr/bin/env node
import { createMascot, getState, STATES, renderAscii } from '../src/bear.mjs';

const help = `Accessor · a little bear, a little company.

Usage: node bin/accessor-mascot.mjs [options]

  --demo                 Tour all fifteen moods
  --state <name>         Start in a particular mood (default: idle)
  --list                 List moods and their movements
  --ascii                Use portable character art
  --no-color             Use monochrome character art
  --color <mode>         truecolor or 256
  --reduced-motion       Static poses, immediate state changes
  --fps <number>         Frames per second, 1–60 (default: 12)
  --duration <seconds>   Close automatically after this many seconds
  --snapshot             Print one portable pose and exit
  --help                 Show this help

Keys: 1–9 / a–f choose a mood · arrows browse · space pause · t tour · q quit
Node.js 18 or later. No dependencies, installation, or network required.
`;
try {
  const options={}; let snapshot=false;
  const args=process.argv.slice(2);
  for(let i=0;i<args.length;i++) {
    const arg=args[i];
    if(arg==='--help'||arg==='-h'){process.stdout.write(help);process.exit(0);}
    if(arg==='--list'){process.stdout.write(STATES.map(s=>`${s.id.padEnd(13)} ${s.motion}`).join('\n')+'\n');process.exit(0);}
    if(arg==='--demo')options.demo=true;
    else if(arg==='--ascii')options.ascii=true;
    else if(arg==='--no-color')options.color=false;
    else if(arg==='--reduced-motion')options.reducedMotion=true;
    else if(arg==='--snapshot')snapshot=true;
    else if(['--state','--color','--fps','--duration'].includes(arg)){
      const value=args[++i]; if(value===undefined)throw new Error(`${arg} needs a value.`);
      options[arg.slice(2)]=['--fps','--duration'].includes(arg)?Number(value):value;
    } else throw new Error(`Unknown option: ${arg}. Use --help.`);
  }
  getState(options.state||'idle');
  if(options.color && !['truecolor','256'].includes(options.color))throw new Error('--color must be truecolor or 256.');
  if(options.duration!==undefined&&(!Number.isFinite(options.duration)||options.duration<0))throw new Error('--duration must be a non-negative number.');
  if(snapshot){process.stdout.write(renderAscii(options.state||'idle',0).map(s=>s.trimEnd()).join('\n')+'\n');}
  else await createMascot(options).start().done;
} catch(error) {
  process.stderr.write(`accessor: ${error.message}\n`);process.exitCode=1;
}
