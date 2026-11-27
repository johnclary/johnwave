#!/usr/bin/env python3
"""Hold a chord into the standalone app without a MIDI keyboard.

Opens a virtual MIDI port named "johnwave keys". Start this first, then:
    cargo dev --midi-input "johnwave keys"
Press Enter to hold the chord, Enter again to release it. Ctrl-C to quit.

Needs: pip install mido python-rtmidi
"""
import argparse

import mido

PORT_NAME = "johnwave keys"

parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
parser.add_argument("--notes", default="48,51,55,58",
                    help="comma-separated MIDI note numbers (default: Cm7, C3 Eb3 G3 Bb3)")
parser.add_argument("--velocity", type=int, default=100)
args = parser.parse_args()
notes = [int(n) for n in args.notes.split(",")]

with mido.open_output(PORT_NAME, virtual=True) as port:
    print(f'"{PORT_NAME}" is open. Enter = chord on/off, Ctrl-C = quit.')
    held = False
    try:
        while True:
            input()
            held = not held
            kind = "note_on" if held else "note_off"
            for n in notes:
                port.send(mido.Message(kind, note=n, velocity=args.velocity if held else 0))
            print("on" if held else "off")
    except (KeyboardInterrupt, EOFError):
        pass
    finally:
        for n in notes:
            port.send(mido.Message("note_off", note=n, velocity=0))
        port.send(mido.Message("control_change", control=123, value=0))
        print("\nreleased, bye")
