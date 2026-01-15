use ringbuf::{HeapCons, traits::{Consumer, Observer}};
use webrtc_vad::Vad;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

const MAX_SILENCE: usize = 50; // frames of silence before ending utterance

fn resample_to_16khz(audio: &[f32], source_rate: u32) -> Vec<f32> {
    if source_rate == 16000 {
        return audio.to_vec();
    }

    let ratio = 16000.0 / source_rate as f32;
    let output_len = (audio.len() as f32 * ratio) as usize;
    let mut resampled = Vec::with_capacity(output_len);

    for i in 0..output_len {
        let src_idx = i as f32 / ratio;
        let src_idx_floor = src_idx.floor() as usize;
        let src_idx_ceil = (src_idx_floor + 1).min(audio.len() - 1);
        let frac = src_idx - src_idx_floor as f32;

        let sample = audio[src_idx_floor] * (1.0 - frac) + audio[src_idx_ceil] * frac;
        resampled.push(sample);
    }

    resampled
}

pub fn run_vad(
    mut audio: HeapCons<f32>,
    source_rate: u32,
    muted: Arc<AtomicBool>,
    mut on_utterance: impl FnMut(Vec<f32>),
) {
    let mut vad = Vad::new();
    vad.set_mode(webrtc_vad::VadMode::VeryAggressive);
    vad.set_sample_rate(webrtc_vad::SampleRate::Rate16kHz);

    let mut buffer = Vec::new();
    let mut silence = 0;
    let mut speaking = false;
    let mut speaking_len = 0;
    let frame_size: usize = ((source_rate / 100) * 3) as usize;

    loop {
        // Skip processing when muted (AI is speaking)
        if muted.load(Ordering::Relaxed) {
            // Clear any accumulated buffer when muted
            if speaking {
                buffer.clear();
                speaking_len = 0;
                speaking = false;
                silence = 0;
            }
            // Still consume audio to prevent buffer overflow
            if audio.occupied_len() >= frame_size {
                for _ in 0..frame_size {
                    audio.try_pop();
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            continue;
        }

        if audio.occupied_len() < frame_size {
            std::thread::sleep(std::time::Duration::from_millis(5));
            continue;
        }

        let mut frame = Vec::with_capacity(frame_size);
        for _ in 0..frame_size {
            frame.push(audio.try_pop().unwrap());
        }

        let resampled_frame = resample_to_16khz(&frame, source_rate);

        let frame_i16: Vec<i16> = resampled_frame.iter()
            .map(|x| (x * i16::MAX as f32) as i16)
            .collect();

        let speech = vad.is_voice_segment(&frame_i16);

        if speech == Ok(true) {
            if speaking_len > 20 {
                speaking = true;
            }

            speaking_len += 1;
            silence = 0;
            buffer.extend(resampled_frame);
        } else if speaking {

            silence += 1;
            buffer.extend(resampled_frame);

            if silence >= MAX_SILENCE {
                if buffer.len() >= 16000 { // 1 second
                    on_utterance(buffer.clone());
                }
                buffer.clear();
                speaking = false;
                speaking_len = 0;
                silence = 0;
            }
        } else {
            speaking_len = 0;
        }
    }
}
