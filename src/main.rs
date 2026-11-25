// Standalone dev app: runs Johnwave outside a DAW, using CoreAudio for sound
// and the default MIDI input for notes. `cargo dev --help` lists the options.

use johnwave::Johnwave;
use nice_plug::prelude::*;

fn main() {
    nice_export_standalone::<Johnwave>();
}
