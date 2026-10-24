# Lightweight synthesized chimes

The 0.1.22 prototype replaces the two-wave high bell with a warmer struck-bar tone. Audition quality remains a user decision; numerical checks establish stability, not realism.

| Technique | Existing work | Fit here |
|---|---|---|
| Additive / modal synthesis | [CMU additive synthesis](https://www.cs.cmu.edu/~15322/book/ch03/index.html); [Stanford STK ModalBar documentation](https://chuck.stanford.edu/doc/reference/ugens-stk.html#ModalBar) and [implementation](https://github.com/thestk/stk/blob/master/src/ModalBar.cpp) | Chosen: a small set of independently damped modes models the ringing after a soft strike. Our closed-form sum needs no external engine or excitation recording. |
| Karplus–Strong | [Stanford explanation](https://xenon.stanford.edu/~blynn/sound/karplusstrong.html) and [Julius O. Smith's physical audio text](https://www.dsprelated.com/freebooks/pasp/Electric_Guitars.html) | Useful for later plucked-string timbres; delay/feedback/tuning machinery is unnecessary for this struck-bar prototype. |

These sources establish the methods. The small Rust implementation and tuning are our own; no STK code, raw excitation file, preset bank or library is incorporated.

## Current prototype

- C4–D4–E4–G4–A4 pentatonic bank, one octave below the previous tone.
- Four damped sinusoidal modes: approximate ratios 1, 4, 10 and 18. Upper modes decay sooner than the fundamental. Per-mode cosine attacks avoid an abrupt onset; a cosine release reaches zero smoothly.
- Duration 320 ms (previously 160 ms). Bank is generated once on the existing audio worker. The playback loop still mixes cached PCM with the same overlapping voices, limiter and primed driver queue. Longer tails can keep more voices active during a sweep; the existing 32-voice cap applies.
- Five mono float32 tables at 48 kHz contain **307,200 bytes (300 KiB)** of PCM, versus 153,600 bytes previously. This is table payload, not total application memory. No new dependency, resident thread, filesystem audio source or idle polling is introduced.
- Configuration and volume settings stay compatible. Instrument choice remains a possible follow-up; this revision changes the single current timbre.

## Review

Run `cargo run --example audio_preview --locked --offline` to write two 3.2-second WAV comparisons into `.local/audio-review`. The files are offline listening artifacts only: the application neither loads nor packages them. Generation never opens an audio device. Same volume and event timing; the new bank intentionally uses a lower register. Each file contains isolated notes, then a rapid overlapping sweep. The example reports peaks; it does not certify equal perceived loudness.

Automated checks cover finite/bounded tables, exact zero endpoints, quiet boundary samples, the decaying tail and fundamental tuning. Existing checks cover overlap without stolen tails, saturation and continuous PCM across output chunks. Listen manually for tone, attack, loudness, rapid-sweep clutter and driver-specific crackle; adjust using that feedback.
