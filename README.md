# johnwave

A wave-morphing synth plugin (CLAP + VST3), built in Rust with [nice-plug](https://crates.io/crates/nice-plug) and an egui UI.

![synth-image](/docs/images/johnwave_1.jpg)

## Getting started

* Check out the [user manual](/docs/user_manual.md)

## Requirements

- Stable Rust, installed via [rustup](https://rustup.rs)

## Test

```sh
cargo test
```

Unit tests live alongside the code in `src/*.rs`.

## Build

```sh
cargo xtask bundle johnwave --release
```

This writes the plugin bundles to:

- `target/bundled/johnwave.vst3`
- `target/bundled/johnwave.clap`

Leave off `--release` for a faster, unoptimised debug build.

## UI dev loop (standalone)

For UI work you can run johnwave as its own app, without a DAW:

```sh
cargo dev
```

This opens the plugin window and plays through CoreAudio. Options go after the
command, e.g. `cargo dev --tempo 90`; `cargo dev --help` lists them all.

No MIDI input is connected unless you name one, by its exact device name:

```sh
cargo dev --midi-input "Oxygen 61"
```

If the name doesn't match, the app still opens but falls back to a silent mode
(no audio or MIDI), with the error only in the terminal. To list the MIDI devices
instead, run `cargo dev --backend core-audio --midi-input ""`.

To rebuild and relaunch on every save, use [watchexec](https://github.com/watchexec/watchexec)
(`brew install watchexec`):

```sh
watchexec -n -r -e rs -- cargo dev --midi-input "Oxygen 61"
```

`-n` passes the arguments straight through. Without it watchexec goes through a
shell that splits `"Oxygen 61"` at the space.

The standalone app uses the same synth and editor code as the plugin, but nothing
sits around it the way a DAW would:

- the tempo is fixed and there's no song position, so tempo-synced LFOs can't lock to a playing song
- nothing is saved, so every relaunch starts from default settings
- nothing changes parameters from outside (no automation)
- it's a debug build, so it's slower than the release plugin

Use it for layout, drawing, knob feel and the scope. Check automation, tempo
sync, saving/reloading and performance in Ableton with a bundled build.

## Deploy (macOS)

Copy the bundles into your user plug-in folders:

```sh
mkdir -p ~/Library/Audio/Plug-Ins/VST3 ~/Library/Audio/Plug-Ins/CLAP
cp -R target/bundled/johnwave.vst3 ~/Library/Audio/Plug-Ins/VST3/
cp -R target/bundled/johnwave.clap ~/Library/Audio/Plug-Ins/CLAP/
```

Then rescan in your DAW. In Ableton: **Settings → Plug-Ins → Rescan**.

For quick iteration you can skip copying: point Ableton's custom VST3
folder at `target/bundled` and just rebuild + rescan.

## Todo

- show mod adjustments in readout
- second lfo and second mod env (and/or use amp env as a mod source)
- ability to type in param values (with right click?)
- a second filter
- more mod targets: porta glide speed, amp env (adsr)
- 2× oversampling around the filter, to cut back on Drive's aliasing turns
