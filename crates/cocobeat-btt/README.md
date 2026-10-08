# Fixed BTT binding

This crate owns one 48 kHz mono BTT instance with the existing 1024 FFT / 8 overlap / 15 filter / 1024 OSS / 1024 threshold / 1024 CBSS configuration, no callbacks and no public parameter setters

The six C translation units, six required headers and LICENSE under vendor/btt are unchanged files from [upstream c039090f1af771092d95c3ffc402e557940f7384](https://github.com/michaelkrzyzaniak/Beat-and-Tempo-Tracking/tree/c039090f1af771092d95c3ffc402e557940f7384), licensed under MIT; the Rust wrapper, fixed C shim and build integration are MPL-2.0

Windows builds require the Visual Studio C++ Clang compiler component because upstream uses C99 variable length arrays; cc selects clang-cl and the Rust target's MSVC ABI, maps POSIX random to C rand for the unused upstream statistical RNG helpers, and the application statically links the resulting code without a runtime compiler dependency

Only the private ffi module permits unsafe Rust; media and other workspace crates retain their existing unsafe forbid boundary, and no raw C handles or configurable BTT API are public

Raw BPM and histogram certainty are diagnostic observations, not calibrated confidence, reliable beat units, musical quality admission or bounded TempoRegion values
