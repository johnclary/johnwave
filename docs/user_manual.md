# johnwave user manual

A small polyphonic synth: one morphing oscillator, a pitch-tracking bitcrusher, a
resonant multi-type filter, two envelopes, an LFO and a mod matrix to route them.

## Parameters

Grouped as they appear in the window, top to bottom.

### Header

| Param | Range | Default | What it does |
|---|---|---|---|
| Voice Mode | Poly / Mono | Poly | Poly plays up to 8 notes at once. Mono plays one note, legato (see below). |

### Oscillator

| Param | Range | Default | What it does |
|---|---|---|---|
| Shape | 0–100% | 0% | Morphs the wave: 0% sine, 33% triangle, 67% saw, 100% square, blending smoothly in between. |

### Tuning

| Param | Range | Default | What it does |
|---|---|---|---|
| Coarse | −24 to +24 st | 0 | Transpose in semitones. |
| Fine | −100 to +100 ct | 0 | Fine tune in cents (100 cents = 1 semitone). |
| Bend | 0–24 st | 2 | How far the pitch bend wheel reaches, each way. |

### Filter

| Param | Range | Default | What it does |
|---|---|---|---|
| Cutoff | 20 Hz – 20 kHz | 20 kHz | The frequency the filter works around. |
| Res | 0–100% | 0% | Resonance: a peak at the cutoff, from none to a strong ring. For BP and NOTCH it narrows the band instead. |
| Type | LP / HP / BP / NOTCH | LP | Which part of the sound the filter keeps (see [The filter](#the-filter)). |

### Amp

| Param | Range | Default | What it does |
|---|---|---|---|
| A (Attack) | 1 ms – 5 s | 5 ms | Time to rise to full volume. |
| D (Decay) | 1 ms – 5 s | 300 ms | Time to fall to the sustain level. |
| S (Sustain) | 0–100% | 70% | Level held while the key is down. |
| R (Release) | 1 ms – 10 s | 200 ms | Time to fade out after the key is released. |
| Gain | −48 to 0 dB | −12 dB | Overall output level. |
| Pan | L100 – R100 | C | Position between the left and right speakers. Type `C`, `L30`, `R100` or a number from −100 to 100. |

### Mod Envelope

| Param | Range | Default | What it does |
|---|---|---|---|
| A (Attack) | 1 ms – 5 s | 1 ms | Time to rise to its peak. |
| D (Decay) | 1 ms – 10 s | 500 ms | Time to fall back to zero. |

What it moves is set in the [Mod Matrix](#mod-matrix).

### LFO

| Param | Range | Default | What it does |
|---|---|---|---|
| Shape | 0–100% | Sine | The LFO's wave, morphing sine → triangle → saw → square like the oscillator. |
| Rate | 0.05–20 Hz | 1 Hz | Speed when not synced. Greyed out while SYNC is on. |
| RETRIG | on / off | on | On: each new note restarts the LFO from the start of its cycle. Off: one free-running LFO shared by all notes. |
| SYNC | on / off | off | Follow the DAW's tempo instead of Rate. |
| Sync rate | 4 bars – 1/32 | 1/4 | Cycle length when synced, including dotted and triplet values. Greyed out while SYNC is off. |

What it moves is set in the [Mod Matrix](#mod-matrix).

### Crush

| Param | Range | Default | What it does |
|---|---|---|---|
| ON | on / off | off | Switches the bitcrusher in. While off, its knobs are greyed out and do nothing, and so do mod routes to them. Flipping it fades over 10 ms, so it never clicks. |
| Bits | 1–16 bits, Off | 8 bits | Bit depth. Fewer bits = bigger steps in volume = more grit. 16 shows as Off. |
| Steps | 2–256 steps, Off | 16 steps | How many flat stairs each wave cycle is cut into, following the note's pitch. Fewer = rougher and more chiptune-like. 256 shows as Off. |
| Mix | 0–100% | 100% | Blend between the clean (0%) and crushed (100%) sound. |

See [The bitcrusher](#the-bitcrusher).

### Mod Matrix

Six slots, each a row of `[Source] [Dest] [Amount]`. Click a Source or Dest button to
step to the next choice, right-click for the previous one. A blank button means Off. A
slot does something only once both are set; until then its amount slider is greyed out.

| Param | Range | Default | What it does |
|---|---|---|---|
| Source | Off, LFO, Mod Env, Velocity, Random | Off | What does the moving (see [Modulation](#modulation)). |
| Dest | Off, Shape, Cutoff, Res, Pitch, Bits, Steps, Mix | Off | What gets moved. Bits, Steps and Mix are the Crush knobs. |
| Amount | −100 to +100% | 0 | How far, shown in the destination's unit (like `+2.0 oct` or `−8.0 bits`). Negative flips the direction. |

Hover over or drag any control to see its exact value in the readout at the top right.

## How it works

### Signal path

Each note gets its own voice, and every voice runs the same chain:

```
oscillator → bitcrusher → filter → amp envelope × velocity
```

The voices are added together, scaled by Gain, then panned to the two output channels.
Pan uses an equal-power law, so loudness stays even across the sweep: at centre each
side is at −3 dB, and hard left or right puts the full level on that side only. All
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

### The filter

A 12 dB/octave resonant filter: a state-variable design that stays stable and smooth
even when the cutoff moves every sample, which matters because the envelope and LFO
sweep it. At full resonance it rings strongly but stops just short of self-oscillation,
so it never whistles on its own.

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

Cutoff modulation is measured in octaves, so a given amount sounds the same anywhere in
the range: +1 octave always doubles the cutoff.

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
- **Mod envelope (AD)** rises to a peak and falls back to zero, whether or not the key
  is still held. It's for one-shot movement at the start of a note, such as a filter
  "pluck" or a bright attack that settles into a sine.

### Modulation

Each mod matrix slot connects one source to one destination by an amount. Any source can
go to any destination. Where several slots push the same destination, their amounts add
up, and the result is kept within the destination's range.

Sources:

- **LFO**: swings both ways, −1 to +1.
- **Mod Env**: 0 to 1, rising and falling once per note.
- **Velocity**: 0 to 1, how hard the note was played. Fixed for the length of the note.
- **Random**: the LFO's sample & hold, −1 to +1. A new random level each LFO cycle,
  following the LFO's Rate and SYNC. Each note gets its own levels while RETRIG is on.

Destinations, and what a slot's amount at 100% means for each:

| Dest | Unit | At 100% |
|---|---|---|
| Shape | Shape range | the whole 0–100% |
| Cutoff | octaves | 6 oct (+1 oct doubles the cutoff) |
| Res | Res range | the whole 0–100% |
| Pitch | semitones | 24 st |
| Bits | bits | 15 bits (the whole 1–16) |
| Steps | octaves | 7 oct (+1 oct doubles the steps, −7 takes 256 down to 2) |
| Mix | Crush Mix range | the whole 0–100% |

A positive amount always turns the destination's knob clockwise. For Bits and Steps that
means cleaner, so use a **negative** amount to add crush. Modulation can bring crush in
even with the Bits or Steps knob at Off, as long as Crush is switched ON.

Some starting points:

- Mod Env → Cutoff, positive: a filter pluck on every note.
- LFO → Shape: the wave slowly morphs, like a filter sweep from the source itself.
- Mod Env → Steps, around −4 oct: each note starts chiptune-crunchy and cleans up.
- Velocity → Bits, negative: harder hits get grittier.

**Mod arcs.** A knob with a route to it shows a pink arc along its ring, covering every
position the modulation can push it to, with a small tick marking where the knob itself
is set. Pitch routes show on the Coarse knob.

### Poly and Mono

- **Poly:** up to 8 voices. When all 8 are busy, a new note takes over the oldest voice
  that's already fading out, and failing that the oldest held one. Replaying a key that
  is still sounding reuses its voice instead of doubling it.
- **Mono (legato):** one voice. Playing a new key while another is held changes the
  pitch without restarting the envelopes. Releasing it falls back to the key still held.
  Switching modes releases any notes that are playing.

### LFO sync and retrigger

With SYNC on, the LFO's speed follows the DAW's tempo. With RETRIG off as well, the
shared LFO also locks to the song position while the transport plays, so it lands in the
same place on every bar. With RETRIG on, each note gets its own LFO that starts from the
beginning of its cycle, which suits per-note wobbles that start the same way every time.

### The screens

- **Oscillator screen** (under the readout): what is playing right now, after
  modulation, drawn as tall as the newest note is loud. Recent shapes leave a short
  fading trail. With nothing playing it shows a flat line.
- **Small waves** beside each Shape knob: the shape that knob is set to, without
  modulation.
