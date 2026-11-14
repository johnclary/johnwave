# johnwave

A small polyphonic synth: one morphing oscillator, a resonant low-pass filter, two
envelopes and an LFO.

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
| Cutoff | 20 Hz – 20 kHz | 20 kHz | Low-pass cutoff frequency. |
| Res | 0–100% | 0% | Resonance: a peak at the cutoff, from none to a strong ring. |

### Amp

| Param | Range | Default | What it does |
|---|---|---|---|
| A (Attack) | 1 ms – 5 s | 5 ms | Time to rise to full volume. |
| D (Decay) | 1 ms – 5 s | 300 ms | Time to fall to the sustain level. |
| S (Sustain) | 0–100% | 70% | Level held while the key is down. |
| R (Release) | 1 ms – 10 s | 200 ms | Time to fade out after the key is released. |
| Gain | −48 to 0 dB | −12 dB | Overall output level. |

### Mod Envelope

| Param | Range | Default | What it does |
|---|---|---|---|
| A (Attack) | 1 ms – 5 s | 1 ms | Time to rise to its peak. |
| D (Decay) | 1 ms – 10 s | 500 ms | Time to fall back to zero. |
| To Shape | −100 to +100% | 0% | How far it pushes Shape at its peak. Negative pushes toward sine. |
| To Cutoff | −6 to +6 oct | 0 | How far it moves the cutoff at its peak, in octaves. Negative closes the filter. |

### LFO

| Param | Range | Default | What it does |
|---|---|---|---|
| Shape | 0–100% | Sine | The LFO's wave, morphing sine → triangle → saw → square like the oscillator. |
| Rate | 0.05–20 Hz | 1 Hz | Speed when not synced. Greyed out while SYNC is on. |
| To Shape | 0–100% | 0% | How far the LFO swings the oscillator's Shape, up and down. At 100% it covers the whole range. |
| To Cutoff | −4 to +4 oct | 0 | How far the LFO swings the cutoff, in octaves. |
| RETRIG | on / off | on | On: each new note restarts the LFO from the start of its cycle. Off: one free-running LFO shared by all notes. |
| SYNC | on / off | off | Follow the DAW's tempo instead of Rate. |
| Sync rate | 4 bars – 1/32 | 1/4 | Cycle length when synced, including dotted and triplet values. Greyed out while SYNC is off. |

Hover over or drag any control to see its exact value in the readout at the top right.

## How it works

### Signal path

Each note gets its own voice, and every voice runs the same chain:

```
oscillator → low-pass filter → amp envelope × velocity
```

The voices are added together, scaled by Gain and sent to both output channels. All
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

A 12 dB/octave resonant low-pass: a state-variable design that stays stable and smooth
even when the cutoff moves every sample, which matters because the envelope and LFO
sweep it. At full resonance it rings strongly but stops just short of self-oscillation,
so it never whistles on its own.

Cutoff modulation is measured in octaves, so a given amount sounds the same anywhere in
the range: +1 octave always doubles the cutoff.

### Envelopes

- **Amp envelope (ADSR)** sets each note's volume over time, multiplied by how hard you
  played the note (velocity). Attack rises from wherever the level currently is, so
  retriggering a note never clicks. Decay and release fall exponentially, and their
  times mean roughly how long it takes to get 99% of the way.
- **Mod envelope (AD)** rises to a peak and falls back to zero, whether or not the key
  is still held. It's for one-shot movement at the start of a note, such as a filter
  "pluck" or a bright attack that settles into a sine.

### Modulation

Each modulation knob connects one source to one destination. Where several push the same
destination, their amounts add up, and the result is kept within the destination's range.

- **Shape** = the Shape knob + Mod Env × To Shape + LFO × To Shape, kept between 0% and
  100%.
- **Cutoff** = the Cutoff knob, moved by the Mod Env and LFO amounts in octaves.

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
