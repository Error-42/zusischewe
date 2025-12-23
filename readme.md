# Zusischewe

A program to cause chaos in Zusi.

It's botched together and doesn't really work very well, but in case you want to try and use it, go ahead.

The main features of the program are:

- delay the entry of trains into the simulation (e.g. `--uniform-probability`, `--uniform_maximum`)
- make trains accelerate (and decelerate) slower (e.g. `--friction`)
- duplicate trains for more traffic (`--duplicate`)

**Make a backup of `_ZusiData` before usage. This shouldn't be required, but I don't trust myself to not have bugs.**

Questions nobody asked:

- Does the program understand what the personal data directory is? – No.
- Does the program just create a renamed copy of the files and modify the data in-place? – Yes.
- Does this cause data loss? – Usually not.
- Should you trust this to not cause data loss and not make a backup of the data? – No.
- Does the program understand the files it works with and the connections between them? – No.
- Does the program use how the official timetables are laid out? – Yes.
- Does this mean it will break very easily? – Yes.
- Is this a command line application? – Yes.
- Should I know what a command line application is? – To use the program, yes.
- Is it really easy to deadlock Zusi with this? – Yes.
- Does Zusi routesetting just break sometimes with this? – Yes.

For usage, run with `--help`.
