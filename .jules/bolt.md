## 2025-05-15 - [Consolidated Audio Normalization]
**Learning:** Audio normalization was performing 4 full passes over the sample buffer (Peak, RMS, Gain, then another Peak scan). By combining Peak/RMS and mathematically deriving the post-gain peak (since target peaks are <= 1.0, clamping is a no-op for the peak), we can reduce this to 2 passes (or 1 if no gain is applied).
**Action:** Always look for opportunities to fuse multiple scans of the same large buffer. For normalization, the second peak scan is redundant if the gain application is bounded by 1.0.

## 2025-05-15 - [Regex-based Filler Removal]
**Learning:** The previous O(N*M) filler removal was extremely inefficient for long dictations, performing full string allocations and lowercase conversions for every filler word. A single-pass regex using `OnceLock` is significantly faster and more memory-efficient.
**Action:** Use `regex` with `OnceLock` for multi-pattern replacement in performance-critical text processing instead of nested loops.

## 2025-05-15 - [On-the-fly Audio Resampling]
**Learning:** Recording raw high-frequency stereo audio (48kHz) consumes 6x more memory than the ASR engine needs (16kHz mono) and adds significant end-of-dictation latency due to batch processing. State-aware chunked resampling in the audio callback eliminates this delay.
**Action:** Move mandatory audio format conversions into the live callback. Use stateful resamplers to handle phase and frame boundaries correctly across chunks.

## 2025-05-15 - [GDI Object Caching in Overlay]
**Learning:** The recording overlay was creating and deleting ~800 GDI objects (brushes/regions) per second during active animations. This creates unnecessary kernel-mode transitions and syscall overhead.
**Action:** Cache static GDI resources like solid brushes and regions that only change on window resize. Use `OverlayWindowState` to store these and ensure proper cleanup in `free_overlay_state`.

## 2025-05-15 - [Zero-copy ASR Padding]
**Learning:** The Moonshine engine requires audio in 1280-sample chunks. Cloning the entire buffer just to add trailing zeros is wasteful when the buffer is already aligned or can be borrowed.
**Action:** Use `std::borrow::Cow` for buffer conditioning before inference. This allows bypassing allocations for correctly-sized inputs, saving time and memory on the hot path.
