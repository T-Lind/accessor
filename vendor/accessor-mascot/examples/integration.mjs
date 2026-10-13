import { createMascot } from '../src/bear.mjs';

// Replace the simulated task with Accessor's real lifecycle events.
const bear = createMascot({ state: 'greeting' }).start();
const timers = [];
for (const [delay, state] of [[1500,'thinking'],[4000,'working'],[7000,'success'],[9000,'celebrating'],[12000,'idle']]) {
  timers.push(setTimeout(() => bear.setState(state), delay));
}
await bear.done;
timers.forEach(clearTimeout);
