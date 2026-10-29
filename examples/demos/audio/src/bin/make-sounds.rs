//! Writes the audio demo's WAV files from code, so they carry no licence:
//! tones, noise bursts, loops and a short piece of music (an intro and its
//! loop at one rate, and one clip with a loop region). Every file is mono,
//! 16-bit, at 44.1 kHz, and the noise is seeded, so a run writes the same
//! bytes each time on one platform (another's maths library may round a
//! few samples' last bit differently).
//!
//! `cargo run -p audio-demo --bin make-sounds`

use std::f32::consts::{PI, TAU};
use std::path::Path;

const RATE: u32 = 44_100;

fn main() {
  let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
  let files: [(&str, Vec<f32>); 22] = [
    ("sounds/tick.wav", tick()),
    ("sounds/click.wav", click()),
    ("sounds/voice.wav", voice()),
    ("sounds/step-1.wav", step(0)),
    ("sounds/step-2.wav", step(1)),
    ("sounds/step-3.wav", step(2)),
    ("sounds/step-4.wav", step(3)),
    ("sounds/bell.wav", bell()),
    ("sounds/hum.wav", hum()),
    ("sounds/chirp.wav", chirp()),
    ("sounds/whistle.wav", whistle()),
    ("sounds/trill.wav", trill()),
    ("sounds/warble.wav", warble()),
    ("sounds/coo.wav", coo()),
    ("sounds/tseep.wav", tseep()),
    ("sounds/buzz.wav", buzz()),
    ("sounds/wind.wav", wind()),
    ("sounds/drone.wav", drone()),
    ("sounds/beep.wav", beep()),
    ("music/theme-intro.wav", theme_intro()),
    ("music/theme-loop.wav", theme_loop()),
    ("music/night.wav", night()),
  ];
  let mut total = 0;
  for (path, samples) in files {
    let bytes = wav(&samples);
    total += bytes.len();
    let file = assets.join(path);
    if let Some(folder) = file.parent()
      && let Err(error) = std::fs::create_dir_all(folder)
    {
      eprintln!("{}: {error}", folder.display());
      std::process::exit(1);
    }
    if let Err(error) = std::fs::write(&file, &bytes) {
      eprintln!("{}: {error}", file.display());
      std::process::exit(1);
    }
    println!("{path}: {:.2} s, {} bytes", seconds(&samples), bytes.len());
  }
  println!("{total} bytes in all");
}

/// A mono 16-bit PCM WAV file of `samples` at [`RATE`]
fn wav(samples: &[f32]) -> Vec<u8> {
  let data = (samples.len() * 2) as u32;
  let mut bytes = Vec::with_capacity(44 + data as usize);
  bytes.extend_from_slice(b"RIFF");
  bytes.extend_from_slice(&(36 + data).to_le_bytes());
  bytes.extend_from_slice(b"WAVEfmt ");
  bytes.extend_from_slice(&16u32.to_le_bytes());
  bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
  bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
  bytes.extend_from_slice(&RATE.to_le_bytes());
  bytes.extend_from_slice(&(RATE * 2).to_le_bytes()); // bytes per second
  bytes.extend_from_slice(&2u16.to_le_bytes()); // bytes per frame
  bytes.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
  bytes.extend_from_slice(b"data");
  bytes.extend_from_slice(&data.to_le_bytes());
  for sample in samples {
    let value = (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16;
    bytes.extend_from_slice(&value.to_le_bytes());
  }
  bytes
}

fn seconds(samples: &[f32]) -> f32 {
  samples.len() as f32 / RATE as f32
}

/// The number of samples in `seconds`
fn len(seconds: f32) -> usize {
  (seconds * RATE as f32).round() as usize
}

/// `seconds` of `f(t)`
fn render(seconds: f32, f: impl Fn(f32) -> f32) -> Vec<f32> {
  (0..len(seconds))
    .map(|i| f(i as f32 / RATE as f32))
    .collect()
}

fn sine(hz: f32, t: f32) -> f32 {
  (TAU * hz * t).sin()
}

/// 0 to 1 over `seconds`, then 1
fn ramp(t: f32, seconds: f32) -> f32 {
  (t / seconds).min(1.0)
}

/// Scaled so its loudest sample is `peak`
fn normalized(mut samples: Vec<f32>, peak: f32) -> Vec<f32> {
  let loudest = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
  if loudest > 0.0 {
    for sample in &mut samples {
      *sample *= peak / loudest;
    }
  }
  samples
}

/// The last `seconds` faded to silence, so a one-shot ends without a click
fn faded_out(mut samples: Vec<f32>, seconds: f32) -> Vec<f32> {
  let fade = len(seconds).min(samples.len());
  let start = samples.len() - fade;
  for (i, sample) in samples[start..].iter_mut().enumerate() {
    *sample *= 1.0 - (i + 1) as f32 / fade as f32;
  }
  samples
}

/// A seeded noise source: the same numbers on every run
struct Noise(u32);

impl Noise {
  /// White noise from -1 to 1
  fn next(&mut self) -> f32 {
    // xorshift32
    self.0 ^= self.0 << 13;
    self.0 ^= self.0 >> 17;
    self.0 ^= self.0 << 5;
    self.0 as f32 / u32::MAX as f32 * 2.0 - 1.0
  }
}

/// A note's frequency from its MIDI number (69 is A4, 440 Hz)
fn midi(note: u8) -> f32 {
  440.0 * 2f32.powf((f32::from(note) - 69.0) / 12.0)
}

/// The sfx bus's tick: a short high blip
fn tick() -> Vec<f32> {
  let samples = render(0.06, |t| {
    sine(1760.0, t) * (-t / 0.012).exp() * ramp(t, 0.001)
  });
  normalized(faded_out(samples, 0.005), 0.6)
}

/// The ui bus's click: a brighter, shorter blip with a little noise
fn click() -> Vec<f32> {
  let mut noise = Noise(7);
  let samples: Vec<f32> = (0..len(0.04))
    .map(|i| {
      let t = i as f32 / RATE as f32;
      (sine(2400.0, t) * 0.7 + noise.next() * 0.3) * (-t / 0.006).exp() * ramp(t, 0.0005)
    })
    .collect();
  normalized(faded_out(samples, 0.004), 0.5)
}

/// The voice bus's line: a gliding buzz shaped by moving formants into five
/// vowel-like syllables a second, as a creature mumbling
fn voice() -> Vec<f32> {
  // Each syllable's two formants, in Hz
  const VOWELS: [(f32, f32); 4] = [
    (700.0, 1200.0),
    (300.0, 2300.0),
    (500.0, 900.0),
    (400.0, 2000.0),
  ];
  let mut phase = 0.0f32;
  let samples: Vec<f32> = (0..len(0.8))
    .map(|i| {
      let t = i as f32 / RATE as f32;
      let pitch = 150.0 + 25.0 * sine(1.3, t);
      phase = (phase + pitch / RATE as f32).fract();
      let syllable = (t * 5.0) as usize;
      let (f1, f2) = VOWELS[syllable % VOWELS.len()];
      let gauss = |hz: f32, centre: f32, width: f32| (-((hz - centre) / width).powi(2)).exp();
      let buzz: f32 = (1..=15)
        .map(|k| {
          let hz = k as f32 * pitch;
          let weight = gauss(hz, f1, 150.0) + 0.6 * gauss(hz, f2, 250.0) + 0.04 / k as f32;
          weight * (TAU * k as f32 * phase).sin()
        })
        .sum();
      let shape = (PI * (t * 5.0).fract()).sin().powf(0.6);
      buzz * shape
    })
    .collect();
  normalized(faded_out(samples, 0.01), 0.5)
}

/// One of four footsteps: a thump falling in pitch and a burst of filtered
/// noise, each take lower or brighter than the others
fn step(take: usize) -> Vec<f32> {
  let low = [70.0, 95.0, 120.0, 150.0][take];
  let bright = [0.12, 0.3, 0.5, 0.75][take];
  let scuff = [0.02, 0.03, 0.025, 0.04][take];
  let mut noise = Noise(11 + take as u32);
  let mut filtered = 0.0f32;
  let mut phase = 0.0f32;
  let samples: Vec<f32> = (0..len(0.14))
    .map(|i| {
      let t = i as f32 / RATE as f32;
      let hz = low * (1.0 + 2.0 * (-t / 0.01).exp());
      phase = (phase + hz / RATE as f32).fract();
      let thump = (TAU * phase).sin() * (-t / 0.035).exp();
      filtered += bright * (noise.next() - filtered);
      let burst = filtered * (-t / scuff).exp();
      (thump + burst * 1.5) * ramp(t, 0.001)
    })
    .collect();
  normalized(faded_out(samples, 0.01), 0.7)
}

/// A struck bell: inharmonic partials, the higher ones dying sooner
fn bell() -> Vec<f32> {
  const PARTIALS: [(f32, f32, f32); 5] = [
    (1.0, 1.0, 0.6),
    (2.0, 0.5, 0.4),
    (2.76, 0.6, 0.3),
    (5.4, 0.25, 0.15),
    (8.93, 0.1, 0.08),
  ];
  let samples = render(1.0, |t| {
    let ring: f32 = PARTIALS
      .iter()
      .map(|&(ratio, level, decay)| level * sine(660.0 * ratio, t) * (-t / decay).exp())
      .sum();
    ring * ramp(t, 0.002)
  });
  normalized(faded_out(samples, 0.05), 0.5)
}

/// A low engine hum with a tremolo: whole cycles of each, so it loops
/// without a seam
fn hum() -> Vec<f32> {
  let samples = render(1.0, |t| {
    let tone = sine(110.0, t) + 0.5 * sine(220.0, t) + 0.25 * sine(330.0, t);
    tone * (1.0 + 0.3 * sine(3.0, t))
  });
  normalized(samples, 0.5)
}

/// Two bird chirps a second, rising sweeps with silence around them
fn chirp() -> Vec<f32> {
  let mut phase = 0.0f32;
  let samples: Vec<f32> = (0..len(1.0))
    .map(|i| {
      let t = i as f32 / RATE as f32;
      let (start, length) = if t < 0.5 { (0.1, 0.09) } else { (0.55, 0.12) };
      let u = (t - start) / length;
      if !(0.0..1.0).contains(&u) {
        return 0.0;
      }
      let hz = 2200.0 + 1400.0 * u;
      phase = (phase + hz / RATE as f32).fract();
      (TAU * phase).sin() * (PI * u).sin().powi(2)
    })
    .collect();
  normalized(samples, 0.5)
}

/// A whistled "fee-bee": a high note sliding a little down, then a lower
/// one, with a slight vibrato, once in a 2.2 s loop
fn whistle() -> Vec<f32> {
  // Each note: its start and length in seconds, and the Hz it slides from
  // and to
  const NOTES: [(f32, f32, f32, f32); 2] =
    [(0.4, 0.3, 3100.0, 2950.0), (0.75, 0.32, 2450.0, 2400.0)];
  let mut phase = 0.0f32;
  let samples: Vec<f32> = (0..len(2.2))
    .map(|i| {
      let t = i as f32 / RATE as f32;
      let Some(&(start, length, from, to)) = NOTES
        .iter()
        .find(|&&(start, length, ..)| (start..start + length).contains(&t))
      else {
        return 0.0;
      };
      let u = (t - start) / length;
      let hz = (from + (to - from) * u) * (1.0 + 0.008 * sine(6.0, t));
      phase = (phase + hz / RATE as f32).fract();
      (TAU * phase).sin() * (PI * u).sin()
    })
    .collect();
  normalized(samples, 0.5)
}

/// A trill: fourteen quick pips falling a little in pitch, once in a 2.6 s
/// loop
fn trill() -> Vec<f32> {
  const START: f32 = 0.9;
  const PIPS: usize = 14;
  // A pip every 45 ms, each 26 ms long
  const EVERY: f32 = 0.045;
  const PIP: f32 = 0.026;
  let samples = render(2.6, |t| {
    let since = t - START;
    if since < 0.0 {
      return 0.0;
    }
    let pip = (since / EVERY) as usize;
    let u = (since - pip as f32 * EVERY) / PIP;
    if pip >= PIPS || u >= 1.0 {
      return 0.0;
    }
    let hz = 4600.0 - 500.0 * pip as f32 / PIPS as f32;
    sine(hz, since) * (PI * u).sin().powi(2)
  });
  normalized(samples, 0.5)
}

/// A warble: a tone wavering fast and slow about 2.8 kHz for 0.8 s, once
/// in a 3 s loop
fn warble() -> Vec<f32> {
  const START: f32 = 1.6;
  const LENGTH: f32 = 0.8;
  let mut phase = 0.0f32;
  let samples: Vec<f32> = (0..len(3.0))
    .map(|i| {
      let t = i as f32 / RATE as f32;
      let u = (t - START) / LENGTH;
      if !(0.0..1.0).contains(&u) {
        return 0.0;
      }
      let hz = 2800.0 + 500.0 * sine(17.0, t) + 250.0 * sine(5.0, t);
      phase = (phase + hz / RATE as f32).fract();
      (TAU * phase).sin() * (PI * u).sin()
    })
    .collect();
  normalized(samples, 0.5)
}

/// A dove's coo: three low, soft notes with a breath of noise, once in a
/// 3.4 s loop. Its peak is higher than the others', as a low note sounds
/// quieter than a high one at the same level
fn coo() -> Vec<f32> {
  // Each note: its start and length in seconds, and the Hz it slides from
  // and to
  const NOTES: [(f32, f32, f32, f32); 3] = [
    (0.2, 0.32, 500.0, 560.0),
    (0.62, 0.55, 600.0, 520.0),
    (1.3, 0.38, 520.0, 480.0),
  ];
  let mut noise = Noise(31);
  let mut breath = 0.0f32;
  let mut phase = 0.0f32;
  let samples: Vec<f32> = (0..len(3.4))
    .map(|i| {
      let t = i as f32 / RATE as f32;
      breath += 0.05 * (noise.next() - breath);
      let Some(&(start, length, from, to)) = NOTES
        .iter()
        .find(|&&(start, length, ..)| (start..start + length).contains(&t))
      else {
        return 0.0;
      };
      let u = (t - start) / length;
      let hz = from + (to - from) * u;
      phase = (phase + hz / RATE as f32).fract();
      let tone =
        (TAU * phase).sin() + 0.3 * (2.0 * TAU * phase).sin() + 0.1 * (3.0 * TAU * phase).sin();
      (tone + 0.6 * breath) * (PI * u).sin().powf(1.5)
    })
    .collect();
  normalized(samples, 0.7)
}

/// Three thin "tseep"s: quick sweeps falling from 6.5 to 4 kHz, once in a
/// 1.7 s loop
fn tseep() -> Vec<f32> {
  const STARTS: [f32; 3] = [1.1, 1.22, 1.34];
  const LENGTH: f32 = 0.07;
  let mut phase = 0.0f32;
  let samples: Vec<f32> = (0..len(1.7))
    .map(|i| {
      let t = i as f32 / RATE as f32;
      let Some(start) = STARTS
        .into_iter()
        .find(|&start| (start..start + LENGTH).contains(&t))
      else {
        return 0.0;
      };
      let u = (t - start) / LENGTH;
      let hz = 6500.0 - 2500.0 * u;
      phase = (phase + hz / RATE as f32).fract();
      (TAU * phase).sin() * (PI * u).sin().powi(2)
    })
    .collect();
  normalized(samples, 0.5)
}

/// A soft square-ish buzz pulsing eight times a second; loops seamlessly
fn buzz() -> Vec<f32> {
  let samples = render(0.5, |t| {
    let tone: f32 = [1.0, 3.0, 5.0, 7.0]
      .iter()
      .map(|k| sine(220.0 * k, t) / k)
      .sum();
    tone * (1.0 + 0.5 * sine(8.0, t))
  });
  normalized(samples, 0.4)
}

/// Wind: noise through a low-pass that opens and closes once a loop, its
/// end crossfaded into its start so it loops without a seam
fn wind() -> Vec<f32> {
  let seconds = 1.5;
  let (body, fade) = (len(seconds), len(0.25));
  let mut noise = Noise(23);
  let (mut a, mut b) = (0.0f32, 0.0f32);
  let raw: Vec<f32> = (0..body + fade)
    .map(|i| {
      let t = i as f32 / RATE as f32;
      let swell = 0.5 + 0.5 * (TAU * t / seconds).sin();
      let open = 0.01 + 0.03 * swell;
      a += open * (noise.next() - a);
      b += open * (a - b);
      b * (0.6 + 0.4 * swell)
    })
    .collect();
  let mut samples = raw[..body].to_vec();
  for i in 0..fade {
    let mix = i as f32 / fade as f32;
    samples[i] = raw[i] * mix.sqrt() + raw[body + i] * (1.0 - mix).sqrt();
  }
  normalized(samples, 0.5)
}

/// The hot reload page's drone: a low chord beating once a second
fn drone() -> Vec<f32> {
  let samples = render(1.0, |t| {
    sine(80.0, t) + sine(81.0, t) + 0.6 * sine(120.0, t) + 0.3 * sine(200.0, t)
  });
  normalized(samples, 0.5)
}

/// The hot reload page's beep
fn beep() -> Vec<f32> {
  let samples = render(0.15, |t| {
    sine(880.0, t) * ramp(t, 0.005) * (-t / 0.06).exp()
  });
  normalized(faded_out(samples, 0.01), 0.5)
}

/// A plucked note: a few harmonics dying away
fn pluck(hz: f32, t: f32) -> f32 {
  if t < 0.0 {
    return 0.0;
  }
  let tone = sine(hz, t) + 0.4 * sine(2.0 * hz, t) + 0.15 * sine(3.0 * hz, t);
  tone * (-t / 0.18).exp() * ramp(t, 0.003)
}

/// A soft held chord, rising in over 10 ms and falling out over the last
/// 10 ms of `length`, so chords follow each other without a click
fn pad(notes: &[u8], t: f32, length: f32) -> f32 {
  if !(0.0..length).contains(&t) {
    return 0.0;
  }
  let edge = ramp(t, 0.01).min(ramp(length - t, 0.01));
  let tone: f32 = notes
    .iter()
    .map(|&note| sine(midi(note), t) + 0.2 * sine(2.0 * midi(note), t))
    .sum();
  tone * edge
}

/// `seconds` of `f(t)` played round a loop: what rings past the end is
/// added back at the start, so the loop repeats without a seam
fn circular(seconds: f32, tail: f32, f: impl Fn(f32) -> f32) -> Vec<f32> {
  let body = len(seconds);
  let mut samples = render(seconds + tail, f);
  for i in body..samples.len() {
    samples[i - body] += samples[i];
  }
  samples.truncate(body);
  samples
}

/// The theme's intro, at 120 beats a minute: a C major arpeggio over a
/// swelling chord, one bar long, silent at its end for the loop to follow
fn theme_intro() -> Vec<f32> {
  const NOTES: [u8; 4] = [60, 64, 67, 72];
  let samples = render(2.0, |t| {
    let plucks: f32 = NOTES
      .iter()
      .enumerate()
      .map(|(beat, &note)| pluck(midi(note), t - beat as f32 * 0.5))
      .sum();
    let swell = pad(&[48, 55, 60], t, 2.0) * (t / 2.0) * 0.25;
    plucks * 0.5 + swell
  });
  normalized(faded_out(samples, 0.12), 0.45)
}

/// The theme's loop: C, A minor, F and G, a bar each of two beats, with a
/// bass note and a pluck on each beat
fn theme_loop() -> Vec<f32> {
  const CHORDS: [[u8; 3]; 4] = [[60, 64, 67], [57, 60, 64], [53, 57, 60], [55, 59, 62]];
  const MELODY: [u8; 8] = [72, 76, 69, 72, 65, 69, 67, 71];
  let samples = circular(4.0, 1.0, |t| {
    let chords: f32 = CHORDS
      .iter()
      .enumerate()
      .map(|(bar, notes)| pad(notes, t - bar as f32, 1.0))
      .sum();
    let bass: f32 = CHORDS
      .iter()
      .enumerate()
      .map(|(bar, notes)| pluck(midi(notes[0] - 24), t - bar as f32))
      .sum();
    let melody: f32 = MELODY
      .iter()
      .enumerate()
      .map(|(beat, &note)| pluck(midi(note), t - beat as f32 * 0.5))
      .sum();
    chords * 0.12 + bass * 0.5 + melody * 0.35
  });
  normalized(samples, 0.45)
}

/// The night track, one clip: a second of a low swell played once, then
/// A minor and E minor, a second each, which the definition loops
fn night() -> Vec<f32> {
  let intro = render(1.0, |t| {
    let swell = (PI * t).sin();
    (sine(midi(45), t) + 0.5 * sine(midi(52), t)) * swell
  });
  let looped = circular(2.0, 1.5, |t| {
    let chords = pad(&[57, 60, 64], t, 1.0) + pad(&[52, 55, 59], t - 1.0, 1.0);
    let bells = pluck(midi(76), t) * 0.5 + pluck(midi(71), t - 1.0) * 0.5;
    let bells_soft = pluck(midi(79), t - 0.5) * 0.25 + pluck(midi(74), t - 1.5) * 0.25;
    chords * 0.15 + bells + bells_soft
  });
  let mut samples = normalized(faded_out(intro, 0.01), 0.3);
  samples.extend(normalized(looped, 0.4));
  samples
}
