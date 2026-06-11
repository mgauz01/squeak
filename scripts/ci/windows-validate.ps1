# Windows CI validation: release compile check + fixture-driven ASR smoke/bench.
$ErrorActionPreference = 'Stop'

$fixture = 'tests/fixtures/asr_smoke_16k_mono.wav'
if (-not (Test-Path $fixture)) {
    throw "Missing fixture: $fixture"
}

Write-Host '=== cargo check --release ==='
cargo check --release --features parakeet

Write-Host '=== cargo build --release (squeak bin) ==='
cargo build --release --bin squeak --features parakeet

Write-Host '=== ASR smoke ==='
cargo run --example asr_smoke --release -- $fixture

Write-Host '=== ASR bench ==='
$log = 'asr-bench.log'
cargo run --example asr_bench --release -- $fixture --models moonshine:small,parakeet *>&1 | Tee-Object -FilePath $log
Write-Host "Bench log written to $log"
