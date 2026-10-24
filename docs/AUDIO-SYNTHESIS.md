# Lightweight synthesized chimes

The 0.1.23 editor exposes Soft mallet (the warmer struck-bar tone introduced in 0.1.22), Classic bell (the previous high bell), Plucked strings and Wind chimes. Audition quality remains a user decision; numerical checks establish stability, not realism.

| Technique | Existing work | Fit here |
|---|---|---|
| Additive / modal synthesis | [CMU additive synthesis](https://www.cs.cmu.edu/~15322/book/ch03/index.html); [Stanford STK ModalBar documentation](https://chuck.stanford.edu/doc/reference/ugens-stk.html#ModalBar) and [implementation](https://github.com/thestk/stk/blob/master/src/ModalBar.cpp) | Chosen: a small set of independently damped modes models the ringing after a soft strike. Our closed-form sum needs no external engine or excitation recording. |
| Karplus–Strong | [Stanford explanation](https://xenon.stanford.edu/~blynn/sound/karplusstrong.html) and [Julius O. Smith's physical audio text](https://www.dsprelated.com/freebooks/pasp/Electric_Guitars.html) | Used for Plucked strings: filtered fractional delay with a softened generated excitation, decay and smooth release. |
| Tubular modes | [Lee Hite's chime design guide](https://leehite.org/Chimes.htm), [Rose-Hulman wind chimes acoustics](https://www.rose-hulman.edu/~moloney/Acoustics/WindChimes.htm), and [Julius O. Smith on modal expansion](https://dsprelated.com/freebooks/pasp/Modal_Expansion.html) | Used for Wind chimes: a struck tube with inharmonic modes, not a recording or ambient wind loop. |

These sources establish the methods. The small Rust implementation and tuning are our own; no STK code, raw excitation file, preset bank or library is incorporated.

## Current prototype

- C4–D4–E4–G4–A4 pentatonic bank, one octave below the previous tone.
- Four damped sinusoidal modes: approximate ratios 1, 4, 10 and 18. Upper modes decay sooner than the fundamental. Per-mode cosine attacks avoid an abrupt onset; a cosine release reaches zero smoothly.
- Duration 320 ms (previously 160 ms). Bank is generated once on the existing audio worker. The playback loop still mixes cached PCM with the same overlapping voices, limiter and primed driver queue. Longer tails can keep more voices active during a sweep; the existing 32-voice cap applies.
- Five mono float32 tables at 48 kHz contain **307,200 bytes (300 KiB)** of PCM, versus 153,600 bytes previously. This is table payload, not total application memory. No new dependency, resident thread, filesystem audio source or idle polling is introduced.
- Configuration defaults to Soft mallet. The editor instrument selector persists on Save and applies to the next chime without restarting. Classic bell uses the original high register and 160 ms envelope. Plucked strings uses a 480 ms filtered feedback string with corrected fractional tuning; Wind chimes uses five tube modes (approximately 1, 2.76, 5.40, 8.93, 13.34), 720 ms of ringing, and smooth attack/release. Each highlight strikes one string/tube; no sound is generated autonomously. A selected bank is generated on the audio worker; active voices keep shared references to their own note data so switching never cuts or retunes existing tails. Old note data is released after those voices finish. The default bank is omitted from JSON; an explicitly saved non-default instrument field requires 0.1.23 or newer.

| Selected bank | PCM table payload (5 mono float32 notes, 48 kHz) |
|---|---:|
| Soft mallet | 300 KiB |
| Classic bell | 150 KiB |
| Plucked strings | 450 KiB |
| Wind chimes | 675 KiB |

These figures exclude application/native overhead. Only the selected full bank is kept; old notes can temporarily remain while existing voices finish. Switching instruments regenerates its bank on the audio worker, not in the input/UI handler. All remain behind the existing 32-voice cap. The 0.1.22 Mallet PCM was checked byte-identical after extracting the shared modal helper.

Wind chimes evaluates damped modes using a decaying complex rotation, avoiding trigonometric/exponential calls for every sample. A regression compares it with the closed-form formula below 16-bit PCM resolution. In one offline release run on this PC, bank generation dropped from about 28.5 ms to 1.95 ms. These are local bank-generation timings, not a playback-latency guarantee; the preview example reports them for repeatable inspection.

## Review

Run `cargo run --example audio_preview --locked --offline` to write four 3.5-second WAV comparisons into `.local/audio-review`. The files are offline listening artifacts only: the application neither loads nor packages them. Generation never opens an audio device. Same volume and event timing; the new bank intentionally uses a lower register. Each file contains isolated notes, then a rapid overlapping sweep. The example reports peaks; it does not certify equal perceived loudness.

Automated checks cover finite/bounded tables, exact zero endpoints, quiet boundary samples, the decaying tail and fundamental tuning. Existing checks cover overlap without stolen tails, saturation and continuous PCM across output chunks. Listen manually for tone, attack, loudness, rapid-sweep clutter and driver-specific crackle; adjust using that feedback.
