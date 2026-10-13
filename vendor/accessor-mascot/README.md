# Accessor bear

A friendly little bear for a terminal application: honey fur, a mint scarf,
round ears, tiny paws, and fifteen expressive moods. The pixel art is drawn
in terminal cells using Unicode half blocks and ANSI colours. It is generated
by code; there are no image assets or runtime dependencies.

## Run

With **Node.js 18 or later**, open a terminal in this folder:

```sh
node bin/accessor-mascot.mjs
```

For the complete animated tour:

```sh
node bin/accessor-mascot.mjs --demo --color truecolor
```

Use a monospaced terminal font. An 80 × 24 window shows the bear and the full
mood menu. Narrower windows keep the bear centred; shorter windows switch to
compact character art. Below 44 × 16 the app shows a brief resize message.

**Controls:** `1–9` and `a–f` select moods; arrows browse; `space` pauses;
`t` starts or stops the tour; `q`, Escape, or Ctrl+C exits. The previous screen,
cursor, and terminal input mode are restored on exit.

## The fifteen moods

| Key | State | Movement | Suggested use |
| --- | --- | --- | --- |
| 1 | `idle` | Breathing and a soft blink | Waiting for input |
| 2 | `greeting` | Paw wave | Starting a session |
| 3 | `curious` | Tilted head, lifted paw, floating question mark | Discovering something |
| 4 | `thinking` | Paw at chin, looking aside, drifting dots | Planning or reasoning |
| 5 | `working` | Typing paws and a tiny laptop | Running a task |
| 6 | `focused` | Mint glasses and a measured nod | Careful work or review |
| 7 | `happy` | Smile eyes, soft sway, little heart | Pleasant conversation |
| 8 | `success` | Raised paw, nod, mint check | Task complete |
| 9 | `surprised` | Wide eyes and a tiny hop | Unexpected discovery |
| a | `confused` | Uneven eyes, head tilt, question mark | Clarification needed |
| b | `worried` | Tucked paws and worried brows | A recoverable snag |
| c | `error` | Gentle head shake and a rose cross | A failed operation |
| d | `sleepy` | Heavy eyelids and a paw-covered yawn | Long idle period |
| e | `sleeping` | Slow breathing and floating zzz | Dormant session |
| f | `celebrating` | Dancing paws and coloured confetti | A milestone |

## Transitions

Every pair of moods supports a transition. Ordinary changes use a **480 ms
blink, dip, and settle**. Sleep entry slows to a **760 ms doze**. Waking uses a
**680 ms stretch**. Worried or error entry uses a soft reassuring head movement.
There are no flashes, terminal bells, or alarming red screens.

The old expression closes its eyes before the new pose appears. A new request
during a transition begins from the pose currently on screen. State changes
are interruptible; users are never held behind an animation queue. Animations
run at 12 fps by default and are entirely local.

## Connect it to Accessor

No Accessor source project was available with this request, so this package
provides a standalone terminal host and a separate renderer for integration.

For a full-screen companion:

```js
import { createMascot } from './src/bear.mjs';

const bear = createMascot({ state: 'greeting' }).start();
bear.setState('thinking');
bear.setState('working');
// After your task finishes:
bear.setState('success');
// When Accessor exits:
bear.stop();
```

`createMascot` owns the terminal screen while active. For an existing TUI, use
the pure renderer instead and let Accessor own its layout and input:

```js
import { BearAnimator, renderArt, renderPixels } from './src/bear.mjs';

const clock = performance.now();
const bear = new BearAnimator('idle', clock);
bear.setState('working', performance.now());

// Inside Accessor's existing rendering loop:
const frame = bear.sample(performance.now());
const pixels = renderPixels(frame.state, frame.time, frame.transition);
const terminalRows = renderArt(pixels, { color: 'truecolor' });
// terminalRows: 18 rows, each 40 cells wide (excluding ANSI escapes).
```

The integration example simulates a task sequence:

```sh
node examples/integration.mjs
```

## Accessibility and portability

```sh
node bin/accessor-mascot.mjs --reduced-motion
node bin/accessor-mascot.mjs --ascii
node bin/accessor-mascot.mjs --no-color
node bin/accessor-mascot.mjs --state sleeping --snapshot
```

Reduced motion uses static poses and immediate state changes. ASCII mode uses
portable character art. `NO_COLOR` and `TERM=dumb` select monochrome character
art. A pipe receives one readable static snapshot, without colour or cursor
escapes. Status names and symbols convey meaning in addition to colour.

True colour is selected when the terminal advertises it; otherwise the renderer
uses the 256-colour palette. `--color truecolor` explicitly enables the full
palette. Linux, macOS, and Windows terminals with ANSI support are the intended
targets; the supplied version was checked in the included Linux environment.

## Design notes

The bear keeps its proportions, outline, and scarf through every mood. Its
expressions are kind even when something goes wrong. Raised paws show little
paw pads. The background is the terminal's own; the design works best on a
dark, warm charcoal background.

| Colour | Hex | Purpose |
| --- | --- | --- |
| Honey | `#D89863` | Fur |
| Butter | `#F7C99A` | Highlights |
| Cocoa | `#704A38` | Outline |
| Cream | `#FFE3BC` | Muzzle |
| Mint | `#7FE3BF` | Scarf and positive symbols |
| Rose | `#ED9B89` | Cheeks and paw pads |

`src/bear.mjs` contains the palette, poses, timing, and terminal host.
`scripts/export-frames.mjs` exports deterministic pixel frames for previews.
The preview images use the same pixel data as the terminal; they are not
separately illustrated concepts.

## Verify

```sh
npm test
```

Checks cover all fifteen animated states, all 210 directed state changes,
interrupted transitions, continuous loop timing, reduced motion, terminal
size limits, piped output, and terminal cleanup. MIT licensed.
