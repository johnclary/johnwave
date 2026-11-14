# How the Shape knob morphs the wave

Notes on johnwave's morphing oscillator: how it works, and how it compares with what
other synths do.

## The code

It all happens in `shape()` in `src/osc.rs`:

```rust
pub fn shape(p: f32, dt: f32, morph: f32) -> f32 {
    let pos = morph.clamp(0.0, 1.0) * 3.0;
    let i = (pos as usize).min(2);
    let t = pos - i as f32;

    let (a, b) = match i {
        0 => (sine(p), triangle(p)),
        1 => (triangle(p), saw(p, dt)),
        _ => (saw(p, dt), square(p, dt)),
    };
    a + (b - a) * t
}
```

## The four frames

A "frame" is one cycle of a wave. Ours are **sine, triangle, saw and square**, each a
small function (`sine()`, `triangle()` and so on in the same file). Each takes a phase
`p` from 0 to 1, how far through the cycle we are, and returns the wave's height at that
point, between −1 and +1. All four start at 0 and rise together (they're
phase-aligned).

## How the blending works

The Shape knob (`morph`, 0 to 1) is stretched to 0–3, which splits it into three
segments, each a crossfade between two neighbouring frames:

| Shape | `pos` | Segment `i` | Fading from → to |
|---|---|---|---|
| 0% – 33% | 0 – 1 | 0 | sine → triangle |
| 33% – 67% | 1 – 2 | 1 | triangle → saw |
| 67% – 100% | 2 – 3 | 2 | saw → square |

- **`i`** is which segment you're in. `pos as usize` chops off the decimals (1.5 → 1).
  In Rust, `as` converts between number types, and converting a decimal number to a
  whole number drops the fraction.
- **`t`** is how far through that segment you are, from 0 to 1: the leftover fraction
  (1.5 − 1 = 0.5).
- **`a + (b - a) * t`** is a straight-line crossfade. At `t = 0` it's all `a`, at
  `t = 1` it's all `b`, and at 0.5 it's half of each.
- **`.min(2)`** handles the top of the knob. At 100%, `pos` is exactly 3.0, which would
  make `i` 3, a segment that doesn't exist. `.min(2)` keeps it in the last segment with
  `t = 1`, which is pure square.

Only two frames ever sound at once. You never hear sine and saw together; the knob walks
along a path, one pair at a time.

## A worked example

Shape at about 17% is halfway from sine to triangle (`pos` = 0.5, `i` = 0, `t` = 0.5).
At phase `p = 0.125` (an eighth of the way through the cycle):

- sine: sin(45°) × 0.816 (its loudness scaling) ≈ 0.577
- triangle: 0.5
- blend: 0.577 + (0.5 − 0.577) × 0.5 ≈ **0.539**

This runs once per output sample, per voice, with `p` moving forward each time.

## What keeps the blend from sounding thin

- **Phase alignment.** Because the blend averages the two waves point by point, they
  have to line up. If the triangle peaked while the sine was dipping, the two would
  partly cancel and the blend would come out quieter and thinner.
- **Equal loudness.** Every frame is scaled to the same RMS level, so turning Shape
  changes the timbre, not the volume. That's why the square looks smaller on the screen.
- **PolyBLEP.** The saw's and square's jumps are smoothed precisely, because the formula
  says exactly where they are. This cuts the harsh overtones (aliasing) they'd otherwise
  cause at high notes.

## Compared with a real wavetable

The Shape knob is like a tiny wavetable: frames along a path, with crossfades between
neighbours. A real wavetable synth (Vital, Serum, Massive; historically PPG and Waldorf)
works differently in the details:

- It **stores** its frames as lists of numbers, often 2,048 points per cycle and often
  dozens or hundreds of frames.
- It looks up and interpolates **twice**: between points within a frame, and between
  neighbouring frames (the same crossfade as our `a + (b - a) * t`).
- To avoid aliasing it usually keeps several **pre-filtered copies** of every frame, one
  per octave.

Ours **computes** its four frames from formulas instead:

- **Nothing stored.** There are no tables to keep in memory.
- **Simpler anti-aliasing.** PolyBLEP can smooth the jumps precisely, with no
  pre-filtered copies needed.
- **The downside:** adding a frame means writing a new formula, not just drawing or
  importing a wave.

## How common is this?

A waveform switch (pick sine, saw, square...) is more common, mostly for historical
reasons:

- **Analog oscillators** naturally produce a saw or triangle and derive the other shapes
  with simple circuits, so each wave is a separate output and a switch is the cheapest
  design. Morphing between them takes extra circuitry.
- **Mixers** are the other classic option: turn on several waves at once (the
  Prophet-5's saw plus pulse) or mix them on faders. That's more flexible than a single
  morph path, because you can build any combination.
- **Predictability:** "saw" is easy to remember and recreate; "Shape 43%" is less so.

Where morphing does show up (product examples from memory, not checked):

- **Analog synths with a continuous wave knob**, such as Moog's Voyager and Sub 37 family
  and Sequential's Prophet-6 / OB-6. Many of these **reshape** the wave rather than
  crossfading: a triangle becomes a saw by sliding its peak to one side, a square narrows
  into a pulse.
- **Wavetable synths**, the big digital version (above).
- **Eurorack oscillators**, where "wave morph" or "timbre" knobs are common because
  everything is built around voltage control.

### Trade-offs of our approach

- **The win:** Shape is one smooth control you can modulate. The LFO and Mod Env can move
  it without clicks or jumps, which a switch can't do. That's the real reason to morph.
- **Crossfading versus reshaping:** in-between positions are mixes of two neighbours (50%
  is literally half triangle, half saw), so they sound like blends, not new waves.
  Reshaping gives genuinely new in-between shapes.
- **No stacking:** you can't have saw and square both at full volume, which a mixer
  allows.

### Possible directions

- **A reshape mode:** a second control that changes the waveform's geometry, like the
  analog approach.
- **Pulse width:** for the square end of the range.
- **More frames:** extra shapes along the morph path, moving it closer to a real
  wavetable.
