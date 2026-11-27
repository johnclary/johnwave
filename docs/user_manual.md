# johnwave user manual

A small polyphonic synth: two morphing oscillators with stereo unison, a pitch-tracking
bitcrusher, two resonant multi-type filters, two envelopes, an LFO and a mod matrix to
route them.

## Parameters

Grouped as they appear in the window, top to bottom.

### Header

| Param                  | Range       | Default  | What it does                                                                                                               |
| ---------------------- | ----------- | -------- | -------------------------------------------------------------------------------------------------------------------------- |
| Voice Mode             | Poly / Mono | Poly     | Poly plays up to 8 notes at once. Mono plays one note, legato (see below).                                                 |
| LFO / ENV / VEL / RAND | chips       | none lit | Click one to enter mod mode for that source, click it again to leave. See [Mod mode](#mod-mode). Not saved with the patch. |

### Oscillator

| Param | Range  | Default | What it does                                                                                |
| ----- | ------ | ------- | ------------------------------------------------------------------------------------------- |
| Shape | 0–100% | 0%      | Morphs the wave: 0% sine, 33% triangle, 67% saw, 100% square, blending smoothly in between. |

### Osc 2

| Param  | Range           | Default | What it does                                                                                                                                                           |
| ------ | --------------- | ------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| ON     | on / off        | off     | Switches the second oscillator in. While off, its knobs are greyed out and do nothing, and so do mod routes to them. Flipping it fades over 10 ms, so it never clicks. |
| Shape  | 0–100%          | 0%      | Osc 2's wave, morphing sine → triangle → saw → square like the main Shape.                                                                                             |
| Coarse | −36 to +36 st   | 0       | Osc 2's tuning against osc 1, in semitones.                                                                                                                            |
| Fine   | −100 to +100 ct | 0       | Osc 2's tuning against osc 1, in cents. A few cents off gives a thick, slowly beating sound.                                                                           |
| Mix    | 0–100%          | 50%     | Blend between osc 1 (0%) and osc 2 (100%).                                                                                                                             |

See [Two oscillators](#two-oscillators).

### Unison

| Param  | Range    | Default | What it does                                                                                                                  |
| ------ | -------- | ------- | ----------------------------------------------------------------------------------------------------------------------------- |
| Voices | 1–7      | 1       | How many copies of the oscillators each note plays. 1 is no unison.                                                           |
| Detune | 0–100 ct | 25 ct   | How far the outer copies are detuned from the note, in cents. The others are spaced evenly in between. Greyed out at 1 voice. |
| Width  | 0–100%   | 50%     | How far the copies spread across the speakers. Greyed out at 1 voice.                                                         |

See [Unison](#unison-1).

### Tuning

| Param  | Range           | Default | What it does                                                                                                                   |
| ------ | --------------- | ------- | ------------------------------------------------------------------------------------------------------------------------------ |
| Coarse | −24 to +24 st   | 0       | Transpose in semitones. Moves both oscillators.                                                                                |
| Fine   | −100 to +100 ct | 0       | Fine tune in cents (100 cents = 1 semitone). Moves both oscillators.                                                           |
| Bend   | 0–24 st         | 2       | How far the pitch bend wheel reaches, each way.                                                                                |
| Porta  | Off – 2 s       | Off     | Portamento: how long each new note takes to slide in from the last one, whatever the interval.                                 |
| LEGATO | on / off        | off     | On: only slide when the new key is pressed while another is still held. Off: every note slides. Greyed out while Porta is Off. |

See [Porta](#porta).

### Filter

| Param  | Range                | Default | What it does                                                                                                                                 |
| ------ | -------------------- | ------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| Cutoff | 20 Hz – 20 kHz       | 20 kHz  | The frequency the filter works around.                                                                                                       |
| Res    | 0–100%               | 0%      | Resonance: a peak at the cutoff, from none to a strong ring. For BP and NOTCH it narrows the band instead.                                   |
| Drive  | 0–100%               | 0%      | How hard the sound is pushed into the filter. Thickens and adds grit, and makes high resonance grind instead of ring. 0% is perfectly clean. |
| Type   | LP / HP / BP / NOTCH | LP      | Which part of the sound the filter keeps (see [The filter](#the-filter)).                                                                    |
| Slope  | 12 / 24              | 12      | How steeply it cuts past the cutoff: 12 dB/octave (open) or 24 dB/octave (darker, rounder).                                                  |

The **F1** and **F2** tabs at the end of the knob row switch the panel between Filter 1
and Filter 2. The window remembers which one was showing. Filter 2 has the same Cutoff,
Res, Drive, Type and Slope controls as Filter 1, plus:

| Param | Range    | Default | What it does                                                                                                |
| ----- | -------- | ------- | ----------------------------------------------------------------------------------------------------------- |
| ON    | on / off | off     | Switches Filter 2 into the sound. While it's off its controls grey out and the F2 tab's label dims.         |
| PAR   | on / off | off     | Off: Filter 1 feeds Filter 2 (serial). On: both filter the same sound side by side, mixed 50/50 (parallel). |

Filter 2 starts as a high-pass at 20 Hz, so in serial switching it on changes nothing
until you raise its Cutoff. See [Two filters](#two-filters).

### Amp

| Param       | Range       | Default | What it does                                                                                        |
| ----------- | ----------- | ------- | --------------------------------------------------------------------------------------------------- |
| A (Attack)  | 1 ms – 5 s  | 5 ms    | Time to rise to full volume.                                                                        |
| D (Decay)   | 1 ms – 5 s  | 300 ms  | Time to fall to the sustain level.                                                                  |
| S (Sustain) | 0–100%      | 70%     | Level held while the key is down.                                                                   |
| R (Release) | 1 ms – 10 s | 200 ms  | Time to fade out after the key is released.                                                         |
| Gain        | −48 to 0 dB | −12 dB  | Overall output level.                                                                               |
| Pan         | L100 – R100 | C       | Position between the left and right speakers. Type `C`, `L30`, `R100` or a number from −100 to 100. |

### Mod Envelope

| Param       | Range       | Default | What it does                                                          |
| ----------- | ----------- | ------- | --------------------------------------------------------------------- |
| A (Attack)  | 1 ms – 5 s  | 1 ms    | Time to rise to its peak.                                             |
| D (Decay)   | 1 ms – 10 s | 500 ms  | Time to fall to the sustain level.                                    |
| S (Sustain) | 0–100%      | 0%      | Level held while the key is down. At 0% it's a one-shot attack/decay. |
| R (Release) | 1 ms – 10 s | 500 ms  | Time to fall back to zero after the key is released.                  |

What it moves is set in the [Mod Matrix](#mod-matrix).

### LFO

| Param     | Range                    | Default | What it does                                                                                                            |
| --------- | ------------------------ | ------- | ----------------------------------------------------------------------------------------------------------------------- |
| Shape     | 0–100%                   | Sine    | The LFO's wave, morphing sine → triangle → saw → square like the oscillator.                                            |
| Rate      | 0.05–20 Hz               | 1 Hz    | Speed when not synced. Greyed out while SYNC is on.                                                                     |
| Fade      | Out 10 s – Off – In 10 s | Off     | Fades the LFO in (right of centre) or out (left) over this long from the start of each note. See [LFO fade](#lfo-fade). |
| RETRIG    | on / off                 | on      | On: each new note restarts the LFO from the start of its cycle. Off: one free-running LFO shared by all notes.          |
| SYNC      | on / off                 | off     | Follow the DAW's tempo instead of Rate.                                                                                 |
| Sync rate | 4 bars – 1/32            | 1/4     | Cycle length when synced, including dotted and triplet values. Greyed out while SYNC is off.                            |

What it moves is set in the [Mod Matrix](#mod-matrix).

### Crush

| Param | Range            | Default  | What it does                                                                                                                                                    |
| ----- | ---------------- | -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| ON    | on / off         | off      | Switches the bitcrusher in. While off, its knobs are greyed out and do nothing, and so do mod routes to them. Flipping it fades over 10 ms, so it never clicks. |
| Bits  | 1–16 bits, Off   | 8 bits   | Bit depth. Fewer bits = bigger steps in volume = more grit. 16 shows as Off.                                                                                    |
| Steps | 2–256 steps, Off | 16 steps | How many flat stairs each wave cycle is cut into, following the note's pitch. Fewer = rougher and more chiptune-like. 256 shows as Off.                         |
| Mix   | 0–100%           | 100%     | Blend between the clean (0%) and crushed (100%) sound.                                                                                                          |

See [The bitcrusher](#the-bitcrusher).

### Mod Matrix

Sixteen slots in two columns (1–8 on the left, 9–16 on the right), each a row of
`[Source] [Dest] [Amount]`. Click a Source or Dest button to step to the next choice,
right-click for the previous one. A blank button means Off. A slot does something only
once both are set; until then its amount slider is greyed out.

You can also set up routes straight from the knobs with [Mod mode](#mod-mode), which
fills in these slots for you.

| Param  | Range                                                                                                                      | Default | What it does                                                                                                                                                                                                                       |
| ------ | -------------------------------------------------------------------------------------------------------------------------- | ------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Source | Off, LFO, Mod Env, Velocity, Random                                                                                        | Off     | What does the moving (see [Modulation](#modulation)).                                                                                                                                                                              |
| Dest   | Off, Shape, Shape 2, Osc Mix, Cutoff, Res, Pitch, Pitch 2, Detune, Bits, Steps, Crush Mix, Drive, Cutoff 2, Res 2, Drive 2 | Off     | What gets moved. Shape 2, Osc Mix and Pitch 2 are the Osc 2 knobs, Detune is the Unison knob, Bits, Steps and Crush Mix are the Crush knobs, Drive is Filter 1's Drive knob, and Cutoff 2, Res 2 and Drive 2 are Filter 2's knobs. |
| Amount | −100 to +100%                                                                                                              | 0       | How far, shown in the destination's unit (like `+2.0 oct` or `−8.0 bits`). Negative flips the direction.                                                                                                                           |

Hover over or drag any control to see its exact value in the readout at the top right.

## How it works

### Signal path

Each note gets its own voice, and every voice runs the same chain:

```
osc 1 + osc 2 (blended by Mix), × unison copies spread left/right
  → bitcrusher → filter 1 (→ filter 2, when on) → amp envelope × velocity
```

With Filter 2's PAR on, filters 1 and 2 sit side by side instead, both fed by the
bitcrusher, and their outputs are mixed 50/50.

Each voice is stereo: everything after the oscillators runs once for the left side and
once for the right. Without unison (or with Width at 0) both sides are identical.

The voices are added together, scaled by Gain, then panned. Pan uses an equal-power
law, so loudness stays even across the sweep: at centre each side is at −3 dB, and hard
left or right puts the full level on that side only. With unison spreading the sound,
Pan works as a balance between the two sides. All
processing happens one sample at a time, so modulation is smooth with no stepping.

### The morphing oscillator

Shape slides through four classic waves, sine → triangle → saw → square, and crossfades
between the two neighbours at any position in between. For example, 50% is halfway from
triangle to saw.

Three things keep the morph sounding even:

- **Phase-aligned waves.** All four shapes start at zero and rise together, so their
  fundamentals line up. A blend between two shapes never partly cancels out and turns
  thin.
- **Equal loudness.** Every shape is scaled to the same RMS level, so turning Shape
  changes the timbre, not the volume. That's why the square looks smaller on the screen:
  it has more energy per cycle, so it's turned down to match.
- **Less aliasing.** The saw's and square's sudden jumps are smoothed with PolyBLEP, a
  cheap correction that removes most of the harsh, inharmonic overtones those jumps
  cause at high pitches.

Moving from sine toward square adds harmonics: sine has none, triangle has faint odd
harmonics, saw has all of them, and square has strong odd ones. In practice, Shape is a
brightness control with character, and modulating it (with the LFO or Mod Envelope)
gives movement much like a filter sweep, but from the source itself.

### Two oscillators

Osc 2 is a second copy of the morphing oscillator, tuned relative to osc 1. Bend, the
Tuning knobs and Pitch routes move both together, so they always stay in tune with each
other. Osc 2's own Coarse, Fine and Pitch 2 routes add on top.

Some ways to use it:

- **Detune.** Leave Coarse at 0 and set Fine a few cents off (try +7). The two
  drift in and out of phase, which gives the thick, moving sound of classic leads and
  pads. More cents = faster beating.
- **Layers.** Coarse at +12 or +7 adds an octave or a fifth on top; −12 with a sine
  Shape adds a sub.
- **Two timbres.** Give each oscillator a different Shape and use Mix (or a route to
  Osc Mix) to move between them.
- **Moving detune.** A small LFO → Pitch 2 route wobbles osc 2 alone, so the
  thickness breathes.

Mix is an equal-power crossfade. Two detuned oscillators add up by power, not by
level, so a plain 50/50 blend would dip by about 3 dB in the middle. This way the
loudness stays even across the whole knob.

Each note starts osc 2 at a random point in its cycle. If it always started in step
with osc 1, two oscillators at the same pitch could partly cancel on one note and sound
full on the next.

The bitcrusher comes after the blend, and its Steps follow osc 1's pitch. With osc 2
tuned to another interval, its stairs don't land evenly on its own cycle, which adds a
little extra grit.

### Unison

Unison plays several copies of the oscillators (osc 1 and osc 2 together) for every
note, each slightly detuned and placed at a different point between the speakers. As
the copies drift in and out of phase, the sound gets wide and shimmering: with a saw
and 5–7 voices, that's the classic "supersaw". One note still uses one voice, so you
can play 8-note chords with unison on, and it works in Mono too.

The copies are spaced evenly. The outer two sit at ±Detune and furthest left and right
(as set by Width), and the rest fill in between. With an odd number of voices, one copy
sits right on the note in the centre, which keeps the pitch solid even at wide detunes.
Every copy is shifted the same way for osc 1 and osc 2, so osc 2's tuning against osc 1
stays the same in each copy.

The copies are added by power, so the level stays the same whatever the Voices count:
more voices sound wider and thicker, not louder. Each copy starts each note at a random
point in its cycle. If they all started together, every note would open with a
"whoosh" as they spread apart.

Some tips:

- **Detune**: around 10–25 ct is lush; 40 ct and up gets sour and dramatic. Detune is
  skewed, so the small values get more of the knob.
- **Width** at 0% keeps the thickness but makes it mono, which helps a bass sit in the
  middle of a mix.
- **Mod Env → Detune**, negative, with Sustain at 0: each note starts wide and settles
  tight.

Changing Voices while a note is held can give a small tick, as copies come and go.

### The filter

A resonant state-variable filter that stays stable and smooth even when the cutoff
moves every sample, which matters because the envelope and LFO sweep it. At full
resonance it rings strongly but stops just short of self-oscillation, so it never
whistles on its own.

The Type switch picks which part of the sound it keeps:

- **LP (low-pass)** keeps everything below the cutoff and cuts above it. The classic
  warm, closing-down sound.
- **HP (high-pass)** keeps everything above the cutoff and cuts below it, thinning the
  sound out. With Cutoff at its default 20 kHz it cuts almost everything, so turn
  Cutoff down when you switch to HP.
- **BP (band-pass)** keeps a band around the cutoff and cuts both sides: a thin, nasal
  sound. Resonance makes the band narrower without making it louder.
- **NOTCH** cuts a band around the cutoff and keeps the rest. Sweeping it gives a
  phaser-like hollow. Resonance makes the cut narrower.

All four types come out of the same filter at once, so switching while a note plays
doesn't reset it.

The Slope switch sets how hard it cuts past the cutoff:

- **12** (12 dB/octave): one filter stage. Gentle and open; the default.
- **24** (24 dB/octave): two stages in a row, so the cut is twice as steep: darker,
  rounder lows, the classic "fat" synth filter. Only the second stage resonates, so you
  still get one clean peak, not two stacked up. The peak is a little softer than at 12,
  which is normal for 4-pole filters. Flipping the slope while a note sounds can click
  slightly.

**Drive** pushes the sound into the filter harder, in two ways at once:

- Before the filter, a soft clipper rounds off the peaks and lifts the quieter parts,
  adding harmonics for the filter to shape. Loud peaks stay at the same level, so
  turning Drive up thickens the sound more than it makes it louder.
- Inside the filter, its resonance saturates the way an analog filter's parts do. At
  high Res the peak gets squashed and grinds instead of ringing cleanly.

Try 24 dB, high Res and high Drive for an aggressive, growling sweep, or Mod Env → Drive
for a bark at the start of every note. At high Drive with the filter wide open, very
bright patches can pick up a faint metallic fizz (aliasing); pulling Cutoff down a little
removes it.

Cutoff modulation is measured in octaves, so a given amount sounds the same anywhere in
the range: +1 octave always doubles the cutoff.

### Two filters

Filter 2 is a second copy of the same filter, with its own Cutoff, Res, Drive, Type and
Slope. It's off by default; switch it on with **ON** on the F2 tab.

- **Serial** (PAR off): the sound goes through Filter 1, then Filter 2. Each one cuts
  on top of the other. The classic use is a high-pass into a low-pass: Filter 2's
  high-pass trims the lows while Filter 1's low-pass shapes the top, leaving a band you
  can move from both ends.
- **Parallel** (PAR on): both filters get the same sound and their outputs are mixed
  50/50. Two band-passes at different cutoffs give two resonant humps, like the
  formants of a voice; sweep one to get vowel-like "wah" and "yow" sounds. A low-pass
  with Filter 2 left wide open blends a filtered sound with the full one.

In parallel, the two filters' outputs can partly cancel where they overlap. A low-pass
and a high-pass at the same cutoff leave a dip there rather than a flat line. The FREQ
screen's curve shows this exactly.

Switching Filter 2 on or off, or flipping PAR, fades over 10 ms, so it doesn't click
mid-note. While Filter 2 is off it isn't run at all and costs no CPU.

Filter 2's knobs are mod destinations too (**Cutoff 2**, **Res 2**, **Drive 2**), in the
same units as Filter 1's. In mod mode the F1 and F2 tabs stay clickable, so you can arm
a source, switch to F2 and drag its knobs. A tab whose knobs have routes, but isn't
showing, gets a small dot in the mod colour, so no route is ever out of sight.

### The bitcrusher

The crusher makes the sound lo-fi in two separate ways, one per knob:

- **Bits** snaps every sample to a coarse grid of volume levels, like an old sampler.
  Fewer bits gives bigger steps, which adds grit and a hiss-like edge.
- **Steps** holds each sample for a while before taking the next, turning the wave into
  a staircase. A plain crusher holds for a fixed time, which adds tones that have
  nothing to do with the note and sounds metallic on a synth. Here the hold follows the
  note's pitch (including bend and pitch modulation), so every cycle gets the same number
  of stairs. The added tones are then all harmonics of the note: it stays in tune and
  sounds like a chiptune or lo-fi sampler rather than clanging. 2–4 steps gives hollow,
  square-ish tones; 8–16 is classic chip crunch.

Each voice has its own crusher, so chords stay clean of each other's steps and every
note can be crushed by a different amount (with Velocity or the Mod Env). It comes
before the filter, so lowering Cutoff smooths its edges into something warmer, and
before the amp envelope, so the grit stays the same as a note fades.

Both knobs move smoothly, including in between whole values (7.5 bits, 10.3 steps), so
they're good to sweep with modulation.

### Envelopes

- **Amp envelope (ADSR)** sets each note's volume over time, multiplied by how hard you
  played the note (velocity). Attack rises from wherever the level currently is, so
  retriggering a note never clicks. Decay and release fall exponentially, and their
  times mean roughly how long it takes to get 99% of the way.
- **Mod envelope (ADSR)** works like the amp envelope, but it moves whatever you route
  it to in the mod matrix instead of the volume. With Sustain at 0 (the default) it rises
  to a peak and falls back to zero while the key is still held. That's for one-shot
  movement at the start of a note, such as a filter "pluck" or a bright attack that
  settles into a sine. Turn Sustain up to hold the movement for as long as the key is
  down. Release then sets how long it takes to return to zero after you let go.

### Modulation

Each mod matrix slot connects one source to one destination by an amount. Any source can
go to any destination. Where several slots push the same destination, their amounts add
up, and the result is kept within the destination's range.

Sources:

- **LFO**: swings both ways, −1 to +1.
- **Mod Env**: 0 to 1, following the mod envelope's shape over each note.
- **Velocity**: 0 to 1, how hard the note was played. Fixed for the length of the note.
- **Random**: the LFO's sample & hold, −1 to +1. A new random level each LFO cycle,
  following the LFO's Rate and SYNC. Each note gets its own levels while RETRIG is on.

LFO and Random both follow the LFO's Fade, so with a fade set they ramp in or out over
each note.

Destinations, and what a slot's amount at 100% means for each:

| Dest           | Unit                 | At 100%                                                  |
| -------------- | -------------------- | -------------------------------------------------------- |
| Shape, Shape 2 | Shape range          | the whole 0–100%                                         |
| Osc Mix        | Mix range            | the whole 0–100%                                         |
| Cutoff         | octaves              | 6 oct (+1 oct doubles the cutoff)                        |
| Res            | Res range            | the whole 0–100%                                         |
| Drive          | Drive range          | the whole 0–100%                                         |
| Cutoff 2       | octaves              | 6 oct, like Cutoff                                       |
| Res 2, Drive 2 | Res and Drive ranges | the whole 0–100%                                         |
| Pitch          | semitones            | 24 st (both oscillators)                                 |
| Pitch 2        | semitones            | 36 st (osc 2 only)                                       |
| Detune         | cents                | 100 ct (the whole Detune range)                          |
| Bits           | bits                 | 15 bits (the whole 1–16)                                 |
| Steps          | octaves              | 7 oct (+1 oct doubles the steps, −7 takes 256 down to 2) |
| Crush Mix      | Crush Mix range      | the whole 0–100%                                         |

A positive amount always turns the destination's knob clockwise. For Bits and Steps that
means cleaner, so use a **negative** amount to add crush. Modulation can bring crush in
even with the Bits or Steps knob at Off, as long as Crush is switched ON.

Some starting points:

- Mod Env → Cutoff, positive: a filter pluck on every note.
- Mod Env → Drive, positive: a growl on each note's attack that settles to clean.
- LFO → Shape: the wave slowly morphs, like a filter sweep from the source itself.
- LFO → Pitch, small amount, Fade at In 800 ms: delayed vibrato that arrives once a
  note is held.
- LFO → Cutoff, Fade at Out 1 s: a wobble on the attack that settles to still.
- Mod Env → Steps, around −4 oct: each note starts chiptune-crunchy and cleans up.
- Velocity → Bits, negative: harder hits get grittier.
- Mod Env → Pitch 2: osc 2 starts off-pitch and slides in, a "zap" on every note.

**Mod arcs.** A knob with a route to it shows an arc in the mod colour (currently yellow)
on a thin outer ring just outside its value arc. The arc covers every position all its
routes together can push it to, while the value arc underneath still shows where the
knob itself is set. Even a tiny amount gets a short visible arc, and a mod-colour dot in
the middle of the knob marks it as modulated at a glance. A faint slice of the same
colour fills the knob from that dot out to the arc, so the range is easy to read from a
distance. Pitch routes show on the
Coarse knob, Pitch 2 routes on Osc 2's Coarse, and Detune routes on the Unison Detune
knob.

### Mod mode

Mod mode sets up modulation by dragging knobs, without going through the matrix.

1. Click a chip in the header (**LFO**, **ENV**, **VEL** or **RAND**). It lights up in the mod colour
   and the window switches to an x-ray view of that source:
   - Every knob it can move gets its name in the mod colour, outlined like a chip, so
     you can see at a glance where the source can go.
   - Knobs it already moves show a mod arc (and dot) for **just that source's** reach.
   - Knobs that can't be modulated go dim and ignore the mouse.
2. Drag a target knob up or down to set how much the source moves it. The knob's own
   value doesn't change; you're setting the route's amount, and the readout shows it
   in the destination's unit (like `LFO > Cutoff  +2.0 oct`). Hold Shift for fine control.
   - Hovering a target the source doesn't reach yet shows **drag to add** in the
     readout. The first drag claims the first free matrix slot. If all 16 are used, the
     readout says **Matrix full**.
   - Dragging a knob the source already reaches edits that route.
   - A slot whose amount is dragged back to 0 does nothing, so it counts as free and
     gets reused by the next new route.
3. To remove that source's routes to a knob, hover the knob: its name turns into
   **Remove**. Click it, and it lights up as **Sure?** (the readout says _Click again to
   remove_). Click again to remove. Moving off the knob, dragging it, or waiting a moment
   cancels, so a stray click mid-performance can't lose a route. Double-click doesn't
   remove routes.
4. Click the lit chip again to leave mod mode. (Esc also works in hosts that pass
   keys through to the plugin; Ableton doesn't.)

**The live tick.** While a note plays in mod mode, a short tick in the mod colour rides
across the value arc, showing where the source is pushing that knob right now. It
follows the newest note, like the screen. With the LFO you see it sweep, with ENV rise
and fall, with VEL sit where the key was struck, and with RAND jump each step.

Greyed-out knobs whose section is off (like the Osc 2 knobs with Osc 2 off) can still be
routed in mod mode. They stay dimmer so they read as off, and their routes do nothing
until you switch the section on.

### Poly and Mono

- **Poly:** up to 8 voices. When all 8 are busy, a new note takes over the oldest voice
  that's already fading out, and failing that the oldest held one. Replaying a key that
  is still sounding reuses its voice instead of doubling it.
- **Mono:** one voice. Playing a new key while another is held changes the
  pitch without restarting the envelopes (sliding there with Porta on). Releasing it
  falls back to the key still held.
  Switching modes releases any notes that are playing.

### Porta

Porta (portamento) makes the pitch slide from one note to the next instead of
jumping. The slide is a straight line in semitones, so it sounds even all the way, and
it always takes the Porta time: a semitone and two octaves both arrive on time. Osc 2,
the unison copies and the crusher's Steps all slide along.

- **Mono:** the note slides from wherever the pitch is right now. Change key mid-slide
  and it turns around from that point. Letting go of a key and falling back to one
  still held slides back too. With LEGATO on, only overlapping notes slide and a
  detached note starts on its own pitch: the classic lead setup, where you choose
  per note whether it glides by how you play.
- **Poly:** each new note slides in from the last note played, so chords swoop in.
  With LEGATO on, only notes pressed while other keys are held slide.

The very first note never slides, since there's nothing to slide from.

### LFO sync and retrigger

With SYNC on, the LFO's speed follows the DAW's tempo. With RETRIG off as well, the
shared LFO also locks to the song position while the transport plays, so it lands in the
same place on every bar. With RETRIG on, each note gets its own LFO that starts from the
beginning of its cycle, which suits per-note wobbles that start the same way every time.

### LFO fade

Fade sets how much of the LFO each note gets over time, counted from the moment the note
starts:

- **Centre (Off)**: the full LFO straight away, as without a fade.
- **Right (In)**: the LFO starts at nothing and rises steadily to full over the set time,
  then stays there. The classic use is delayed vibrato, the way a singer or violinist
  lets a held note start to waver.
- **Left (Out)**: the LFO starts at full and falls steadily to nothing, then stays
  still. Good for a wobble or flutter on the attack only.

The knob is skewed, so short fades (under a second or so) get most of its travel.
Double-click to return it to Off. You can also type a value like `in 500 ms`, `out 2 s`
or `off`.

Every note gets its own fade, even with RETRIG off: the shared LFO keeps running, but each
note's share of it still starts from that note's beginning. A legato pitch change in mono
doesn't restart the fade, just as it doesn't restart the envelopes. Fade scales the LFO's
output, so it affects every route from LFO and Random at once, and the live tick in mod
mode shows the faded value.

### The screens

The **oscillator screen** sits under the readout. The **WAVE** and **FREQ** chips
underneath switch between its two views. In each view, the chips along the top switch
its lines on and off. All of them start on, and the view and chips are saved with the
patch.

**WAVE** shows two cycles of the newest note's oscillators right now, after modulation,
drawn as tall as the note is loud. So it swells on the attack and shrinks away on the
release.

- **OSC**: both oscillators blended together, before Crush and the filter, as a bold
  line. Recent shapes leave a short fading trail behind it. The line's colour drifts as
  Shape goes from sine to square. (Each oscillator's own shape is the small wave beside
  its Shape knob.)
- **OUT**: what the synth actually played, after Crush, the filter, unison and any other
  notes, lined up with osc 1's cycles. It's a thin trace in the mod colour. With none of
  those effects in play, it lands right on the OSC line.

With nothing playing, WAVE shows just a faint centre line.

**FREQ** shows low to high frequencies left to right, from 20 Hz to 20 kHz, with
gridlines at 100 Hz, 1 kHz and 10 kHz.

- **FILTER**: the filter's response curve, showing what it lets through at each
  frequency (−36 to +24 dB, with 0 dB at the faint line across). With Filter 2 on it
  shows both filters together, serial or parallel. While a note plays, it follows that
  note's cutoffs and resonances, mod routes and all. Otherwise it shows the knobs.
- **OUT**: a live spectrum of the output behind the curve, as a soft filled hill. It has
  its own scale, from 0 dBFS at the top to −90 dBFS at the bottom.

The screen's chips fade and ignore clicks in mod mode. (The Filter panel's F1 and F2
tabs don't: see [Two filters](#two-filters).)

The **small waves** beside each Shape knob (Osc 1, Osc 2 and the LFO) show the shape
that knob is set to, without modulation. Each sits in a little frame like a tiny copy of
the screen, with a centre line. Osc 2's dims while Osc 2 is off.
