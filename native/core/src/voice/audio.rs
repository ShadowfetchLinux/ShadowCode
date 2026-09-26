//! Audio plumbing for dictation: downmix, resample to whisper's 16 kHz mono,
//! input level, and a small WAV reader/writer. Pure functions, no devices.
use anyhow::{bail, ensure, Context, Result};

/// whisper.cpp expects 16 kHz mono `f32` in [-1, 1].
pub const WHISPER_RATE: u32 = 16_000;

/// Average interleaved frames down to one channel.
pub fn downmix(interleaved: &[f32], channels: usize) -> Vec<f32> {
    match channels {
        0 => Vec::new(),
        1 => interleaved.to_vec(),
        n => interleaved
            .chunks_exact(n)
            .map(|frame| frame.iter().sum::<f32>() / n as f32)
            .collect(),
    }
}

/// Band-limited resampling (Blackman-windowed sinc). When downsampling the
/// cutoff drops to the new Nyquist rate so 48 kHz speech does not alias
/// into the 16 kHz result.
pub fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() || from == 0 || to == 0 {
        return input.to_vec();
    }
    let ratio = to as f64 / from as f64;
    // Cutoff relative to the input's Nyquist frequency, with a little room.
    let cutoff = ratio.min(1.0) * 0.95;
    const ZERO_CROSSINGS: f64 = 12.0;
    let half_width = (ZERO_CROSSINGS / cutoff).ceil() as i64;
    let out_len = ((input.len() as f64) * ratio).round() as usize;
    let last = input.len() as i64 - 1;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let center = i as f64 / ratio;
        let base = center.floor() as i64;
        let (mut acc, mut norm) = (0.0f64, 0.0f64);
        for k in (base - half_width + 1)..=(base + half_width) {
            let distance = center - k as f64;
            let x = distance / half_width as f64;
            if x.abs() >= 1.0 {
                continue;
            }
            let window = 0.42
                + 0.5 * (std::f64::consts::PI * x).cos()
                + 0.08 * (2.0 * std::f64::consts::PI * x).cos();
            let weight = sinc(distance * cutoff) * window;
            // Repeat the edge samples instead of assuming silence.
            acc += input[k.clamp(0, last) as usize] as f64 * weight;
            norm += weight;
        }
        out.push(if norm.abs() > 1e-9 {
            (acc / norm) as f32
        } else {
            0.0
        });
    }
    out
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-9 {
        1.0
    } else {
        let px = std::f64::consts::PI * x;
        px.sin() / px
    }
}

/// Mono samples at any rate → what whisper reads: 16 kHz, clamped, and at
/// least one second long (short clips are padded with silence).
pub fn for_whisper(mono: &[f32], rate: u32) -> Vec<f32> {
    let mut samples = resample(mono, rate, WHISPER_RATE);
    for s in &mut samples {
        *s = s.clamp(-1.0, 1.0);
    }
    let min = WHISPER_RATE as usize + WHISPER_RATE as usize / 10;
    if samples.len() < min {
        samples.resize(min, 0.0);
    }
    samples
}

pub fn i16_to_f32(sample: i16) -> f32 {
    sample as f32 / 32_768.0
}

pub fn f32_to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * 32_767.0).round() as i16
}

/// Root mean square of a block.
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// True when some 30 ms stretches (100 ms in total) are louder than a quiet
/// room. Silence and faint noise are not sent to whisper, which would
/// otherwise invent words for them.
pub fn has_speech(mono: &[f32], rate: u32) -> bool {
    let frame = (rate as usize * 30 / 1000).max(1);
    let needed = (rate as usize / 10).div_ceil(frame);
    mono.chunks(frame)
        .filter(|chunk| chunk.len() == frame && rms(chunk) > SPEECH_RMS)
        .nth(needed.saturating_sub(1))
        .is_some()
}

/// About -45 dBFS.
const SPEECH_RMS: f32 = 0.0056;

/// A 0–1 meter value: -60 dBFS and below read 0, full scale reads 1.
pub fn meter(rms: f32) -> f32 {
    if rms <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * rms.log10();
    ((db + 60.0) / 60.0).clamp(0.0, 1.0)
}

/// 16-bit PCM mono WAV.
pub fn encode_wav(mono: &[f32], rate: u32) -> Vec<u8> {
    let data_len = (mono.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for &s in mono {
        out.extend_from_slice(&f32_to_i16(s).to_le_bytes());
    }
    out
}

/// Read a PCM (8/16/24/32-bit integer) or 32-bit float WAV into mono samples
/// and its sample rate.
pub fn decode_wav(bytes: &[u8]) -> Result<(Vec<f32>, u32)> {
    ensure!(
        bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE",
        "Not a WAV file"
    );
    let mut pos = 12;
    let mut format: Option<(u16, u16, u32, u16)> = None;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let len = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into()?) as usize;
        let start = pos + 8;
        let end = start.saturating_add(len).min(bytes.len());
        let body = &bytes[start..end];
        match id {
            b"fmt " => {
                ensure!(body.len() >= 16, "The WAV format block is too short");
                let mut tag = u16::from_le_bytes([body[0], body[1]]);
                let channels = u16::from_le_bytes([body[2], body[3]]);
                let rate = u32::from_le_bytes(body[4..8].try_into()?);
                let bits = u16::from_le_bytes([body[14], body[15]]);
                if tag == 0xFFFE && body.len() >= 26 {
                    // WAVE_FORMAT_EXTENSIBLE: the sub-format GUID starts with the tag.
                    tag = u16::from_le_bytes([body[24], body[25]]);
                }
                format = Some((tag, channels, rate, bits));
            }
            b"data" => {
                let (tag, channels, rate, bits) =
                    format.context("The WAV data comes before its format")?;
                ensure!(channels > 0 && rate > 0, "The WAV header is invalid");
                let samples: Vec<f32> = match (tag, bits) {
                    (1, 8) => body.iter().map(|&b| (b as f32 - 128.0) / 128.0).collect(),
                    (1, 16) => body
                        .chunks_exact(2)
                        .map(|c| i16_to_f32(i16::from_le_bytes([c[0], c[1]])))
                        .collect(),
                    (1, 24) => body
                        .chunks_exact(3)
                        .map(|c| {
                            (i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 / 8_388_608.0
                        })
                        .collect(),
                    (1, 32) => body
                        .chunks_exact(4)
                        .map(|c| {
                            i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f32 / 2_147_483_648.0
                        })
                        .collect(),
                    (3, 32) => body
                        .chunks_exact(4)
                        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                        .collect(),
                    _ => bail!("Unsupported WAV encoding (format {tag}, {bits}-bit)"),
                };
                return Ok((downmix(&samples, channels as usize), rate));
            }
            _ => {}
        }
        // Chunks are padded to an even length.
        pos = start.saturating_add(len + (len & 1));
    }
    bail!("The WAV file has no audio data")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f32, rate: u32, seconds: f32) -> Vec<f32> {
        (0..(rate as f32 * seconds) as usize)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin() * 0.5)
            .collect()
    }

    /// Amplitude of `freq` in `samples` (single-bin DFT), 0.5 for the tones above.
    fn amplitude(samples: &[f32], freq: f32, rate: u32) -> f32 {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, &s) in samples.iter().enumerate() {
            let phase = 2.0 * std::f64::consts::PI * freq as f64 * i as f64 / rate as f64;
            re += s as f64 * phase.cos();
            im += s as f64 * phase.sin();
        }
        (2.0 * (re * re + im * im).sqrt() / samples.len() as f64) as f32
    }

    #[test]
    fn downmix_averages_frames() {
        assert_eq!(downmix(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
        assert_eq!(downmix(&[0.25, 0.75], 1), vec![0.25, 0.75]);
        assert!(downmix(&[1.0], 0).is_empty());
        // A trailing partial frame is dropped.
        assert_eq!(downmix(&[1.0, 1.0, 1.0], 2), vec![1.0]);
    }

    #[test]
    fn resample_48k_keeps_speech_and_removes_alias() {
        let rate = 48_000;
        let speech = tone(440.0, rate, 1.0);
        let out = resample(&speech, rate, WHISPER_RATE);
        assert_eq!(out.len(), 16_000);
        let kept = amplitude(&out[1000..15_000], 440.0, WHISPER_RATE);
        assert!((kept - 0.5).abs() < 0.02, "440 Hz amplitude {kept}");
        // 11 kHz is above the new Nyquist rate (8 kHz); without filtering it
        // would fold to 5 kHz.
        let high = tone(11_000.0, rate, 1.0);
        let out = resample(&high, rate, WHISPER_RATE);
        let folded = amplitude(&out[1000..15_000], 5_000.0, WHISPER_RATE);
        assert!(folded < 0.02, "aliased energy {folded}");
    }

    #[test]
    fn resample_44k1_and_8k_lengths_and_passthrough() {
        let out = resample(&tone(300.0, 44_100, 2.0), 44_100, WHISPER_RATE);
        assert_eq!(out.len(), 32_000);
        assert!((amplitude(&out[2000..30_000], 300.0, WHISPER_RATE) - 0.5).abs() < 0.02);
        let up = resample(&tone(300.0, 8_000, 1.0), 8_000, WHISPER_RATE);
        assert_eq!(up.len(), 16_000);
        assert!((amplitude(&up[1000..15_000], 300.0, WHISPER_RATE) - 0.5).abs() < 0.02);
        let same = vec![0.1, -0.2, 0.3];
        assert_eq!(resample(&same, 16_000, 16_000), same);
        assert!(resample(&[], 48_000, 16_000).is_empty());
    }

    #[test]
    fn for_whisper_pads_short_clips_and_clamps() {
        let out = for_whisper(&[2.0; 4_800], 48_000);
        assert_eq!(out.len(), 17_600);
        assert!(out.iter().all(|s| (-1.0..=1.0).contains(s)));
        assert_eq!(out[17_000], 0.0);
    }

    #[test]
    fn has_speech_ignores_silence_and_faint_noise() {
        let rate = 16_000;
        assert!(!has_speech(&vec![0.0; rate as usize * 2], rate));
        // Faint hiss around -60 dBFS.
        let hiss: Vec<f32> = (0..rate * 2)
            .map(|i| if i % 2 == 0 { 0.001 } else { -0.001 })
            .collect();
        assert!(!has_speech(&hiss, rate));
        // A 50 ms click is not speech; half a second of voice is.
        let mut click = vec![0.0; rate as usize];
        click[..800].fill(0.3);
        assert!(!has_speech(&click, rate));
        let mut voice = vec![0.0; rate as usize];
        voice.extend(tone(220.0, rate, 0.5));
        assert!(has_speech(&voice, rate));
        assert!(!has_speech(&[], rate));
    }

    #[test]
    fn meter_maps_dbfs() {
        assert_eq!(meter(0.0), 0.0);
        assert_eq!(meter(0.0005), 0.0); // about -66 dBFS
        assert!((meter(1.0) - 1.0).abs() < 1e-6);
        assert!((meter(0.031_622_8) - 0.5).abs() < 0.01); // -30 dBFS
        assert!((rms(&[0.5, -0.5]) - 0.5).abs() < 1e-6);
        assert_eq!(rms(&[]), 0.0);
    }

    #[test]
    fn wav_round_trip_and_formats() {
        let samples: Vec<f32> = tone(200.0, 16_000, 0.1);
        let wav = encode_wav(&samples, 16_000);
        assert_eq!(wav.len(), 44 + samples.len() * 2);
        let (back, rate) = decode_wav(&wav).unwrap();
        assert_eq!(rate, 16_000);
        assert_eq!(back.len(), samples.len());
        assert!(back.iter().zip(&samples).all(|(a, b)| (a - b).abs() < 1e-3));

        // Stereo float with an extra chunk before the data.
        let mut stereo = Vec::new();
        stereo.extend_from_slice(b"RIFF\0\0\0\0WAVEfmt ");
        stereo.extend_from_slice(&16u32.to_le_bytes());
        stereo.extend_from_slice(&3u16.to_le_bytes());
        stereo.extend_from_slice(&2u16.to_le_bytes());
        stereo.extend_from_slice(&44_100u32.to_le_bytes());
        stereo.extend_from_slice(&(44_100u32 * 8).to_le_bytes());
        stereo.extend_from_slice(&8u16.to_le_bytes());
        stereo.extend_from_slice(&32u16.to_le_bytes());
        stereo.extend_from_slice(b"LIST");
        stereo.extend_from_slice(&3u32.to_le_bytes());
        stereo.extend_from_slice(b"abc\0"); // odd length plus pad byte
        stereo.extend_from_slice(b"data");
        stereo.extend_from_slice(&16u32.to_le_bytes());
        for v in [1.0f32, 0.0, -0.5, -0.5] {
            stereo.extend_from_slice(&v.to_le_bytes());
        }
        let (mono, rate) = decode_wav(&stereo).unwrap();
        assert_eq!(rate, 44_100);
        assert_eq!(mono, vec![0.5, -0.5]);

        assert!(decode_wav(b"not audio").is_err());
        let mut odd = encode_wav(&[0.0], 16_000);
        odd[34] = 12; // 12-bit
        assert!(decode_wav(&odd)
            .unwrap_err()
            .to_string()
            .contains("Unsupported"));
    }
}
