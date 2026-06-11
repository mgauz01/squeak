# Test fixtures

## `asr_smoke_16k_mono.wav`

Synthetic 16 kHz mono PCM WAV (~0.5 s, 440 Hz sine tone) used by CI and local Windows smoke/bench examples (`asr_smoke`, `asr_bench`). No external audio license — generated in-repo.

Regenerate:

```bash
python3 -c "
import wave, struct, math
path = 'tests/fixtures/asr_smoke_16k_mono.wav'
rate, duration, freq = 16000, 0.5, 440.0
n = int(rate * duration)
with wave.open(path, 'w') as w:
    w.setnchannels(1); w.setsampwidth(2); w.setframerate(rate)
    w.writeframes(b''.join(struct.pack('<h', int(32767 * 0.25 * math.sin(2 * math.pi * freq * i / rate))) for i in range(n)))
"
```

## `gec_samples.txt`

Sample sentences for `gec_bench` (see `examples/gec_bench.rs`).
