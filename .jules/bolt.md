## 2025-05-15 - [Consolidated Audio Normalization]
**Learning:** Audio normalization was performing 4 full passes over the sample buffer (Peak, RMS, Gain, then another Peak scan). By combining Peak/RMS and mathematically deriving the post-gain peak (since target peaks are <= 1.0, clamping is a no-op for the peak), we can reduce this to 2 passes (or 1 if no gain is applied).
**Action:** Always look for opportunities to fuse multiple scans of the same large buffer. For normalization, the second peak scan is redundant if the gain application is bounded by 1.0.

## 2025-05-15 - [Regex-based Filler Removal]
**Learning:** The previous O(N*M) filler removal was extremely inefficient for long dictations, performing full string allocations and lowercase conversions for every filler word. A single-pass regex using `OnceLock` is significantly faster and more memory-efficient.
**Action:** Use `regex` with `OnceLock` for multi-pattern replacement in performance-critical text processing instead of nested loops.
