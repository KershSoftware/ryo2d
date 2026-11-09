# Particles: speed and limits

## Measured

### Before (the scene-driven system, main 74fd1521)

Machine: Apple M5 Pro, 18 cores, shared with other work. Bench: `cargo bench --bench particles`, 1920x1080 headless, 1,000 interleaved frames over an empty frame, the GPU drained outside the measure, each scene warmed up 240 frames first. The scenes: `particles_10k` is one emitter holding 9,916 particles (cap 10,000, rate 5,000, lifetime 2); `particles_100x100` is 100 emitters holding 9,900 (rate 50, max 100 each); `particles_collide_2k` is one emitter holding 1,983 bouncing on a 64x64 tile map (16 px tiles, border and 8 platforms; its cap is raised to 2,000, as the default 1,000 would hold half).

One-minute load before each run (`uptime`), every one above 3 and so flagged: run 1 8.18, run 2 6.07, run 3 4.84. The load fell from run to run and the figures did not move with it.

Each cell is the step's median and 95th percentile over the empty frame's median, in microseconds (an empty frame's step: about 104).

| Bench | Run 1 | Run 2 | Run 3 |
|-------|-------|-------|-------|
| `particles_10k` | 675.8 / 741.8 | 668.7 / 749.6 | 678.3 / 738.2 |
| `particles_100x100` | 704.2 / 775.1 | 704.3 / 790.0 | 714.0 / 783.1 |
| `particles_collide_2k` | 287.2 / 329.7 | 286.6 / 332.0 | 290.2 / 328.3 |
