//! Cuelume's success/arrival recipes used by React, rendered as quiet PCM cues.
//! Recipe attribution: Daniel Belyi, cuelume (MIT); see NOTICE.
#[derive(Clone, Copy)]
pub(crate) enum Cue {
    Finished,
    Input,
    LinkedActivity,
}
pub(crate) fn play(cue: Cue) {
    #[cfg(all(target_os = "macos", not(test)))]
    std::thread::spawn(move || {
        let directory = crate::data_dir().join("sounds");
        if std::fs::create_dir_all(&directory).is_err() {
            return;
        }
        let path = directory.join(match cue {
            Cue::Finished => "finished.wav",
            Cue::Input => "input.wav",
            Cue::LinkedActivity => "linked-activity.wav",
        });
        if !path.exists() && std::fs::write(&path, wave(cue)).is_err() {
            return;
        }
        let mut command = std::process::Command::new("/usr/bin/afplay");
        command
            .arg(path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        monocode_core::process::configure_child(&mut command);
        let _ = command.status();
    });
    #[cfg(any(not(target_os = "macos"), test))]
    let _ = cue;
}
#[cfg(any(target_os = "macos", test))]
fn wave(cue: Cue) -> Vec<u8> {
    type Layer = (f32, f32, f32, f32, f32, f32);
    type Recipe = (f32, &'static [Layer], f32, f32, f32);
    let (gain, layers, delay, feedback, wet): Recipe = match cue {
        Cue::Finished => (
            0.5,
            &[
                (880., 880., 0., 0.004, 0.09, 0.06),
                (1108.73, 1108.73, 0.06, 0.004, 0.1, 0.06),
                (1318.51, 1318.51, 0.12, 0.004, 0.18, 0.07),
            ],
            0.1,
            0.22,
            0.16,
        ),
        // cuelume 0.2.2 dist/sounds/recipes.js: RECIPES.chime.
        Cue::LinkedActivity => (
            0.5,
            &[
                (1046.5, 1046.5, 0., 0.006, 0.22, 0.09),
                (1568., 1568., 0.09, 0.006, 0.26, 0.08),
            ],
            0.12,
            0.25,
            0.18,
        ),
        Cue::Input => (
            0.44,
            &[
                (220., 440., 0., 0.04, 0.34, 0.055),
                (659.25, 659.25, 0.12, 0.045, 0.32, 0.04),
                (987.77, 987.77, 0.19, 0.045, 0.34, 0.032),
            ],
            0.16,
            0.28,
            0.18,
        ),
    };
    let rate = 48000;
    let mut samples = vec![0f32; rate * 2];
    for &(freq, end, offset, attack, decay, peak) in layers {
        let mut phase = 0.;
        for i in 0..((attack + decay) * rate as f32) as usize {
            let t = i as f32 / rate as f32;
            let envelope = if t < attack {
                0.0001 * (peak / 0.0001).powf(t / attack)
            } else {
                peak * (0.0001 / peak).powf((t - attack) / decay)
            };
            let hz = freq * (end / freq).powf((t / 0.32).min(1.));
            phase += std::f32::consts::TAU * hz / rate as f32;
            let index = (offset * rate as f32) as usize + i;
            samples[index] += phase.sin() * envelope;
        }
    }
    let lag = (delay * rate as f32) as usize;
    let dry = samples.clone();
    for (i, sample) in samples.iter_mut().enumerate() {
        let mut distance = lag;
        let mut amplitude = wet;
        while distance <= i && amplitude > 0.0001 {
            *sample += dry[i - distance] * amplitude;
            amplitude *= feedback;
            distance += lag;
        }
        *sample *= gain * 0.55 * 4.;
    }
    let bytes = (samples.len() * 2) as u32;
    let mut wav = Vec::with_capacity(bytes as usize + 44);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(bytes + 36).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&(rate as u32).to_le_bytes());
    wav.extend_from_slice(&((rate * 2) as u32).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&bytes.to_le_bytes());
    for value in samples {
        wav.extend_from_slice(&((value.clamp(-1., 1.) * 32767.) as i16).to_le_bytes());
    }
    wav
}

#[cfg(test)]
mod tests {
    #[test]
    fn cues_are_bounded_pcm() {
        for cue in [super::Cue::Finished, super::Cue::Input] {
            let wav = super::wave(cue);
            assert_eq!(&wav[..4], b"RIFF");
            assert_eq!(&wav[8..12], b"WAVE");
            assert_eq!(wav.len(), 192044);
            assert!(wav[44..].iter().any(|v| *v != 0));
        }
    }
}
