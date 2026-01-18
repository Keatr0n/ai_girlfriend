use ringbuf::{HeapCons, traits::{Consumer, Observer}};
use webrtc_vad::Vad;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

fn downsample_to_16k_box(input: &[f32], in_rate: u32) -> Vec<f32> {
    let step = in_rate as f32 / 16_000.0;
    let out_len = (input.len() as f32 / step) as usize;

    let mut out = Vec::with_capacity(out_len);

    let mut pos = 0.0f32;

    for _ in 0..out_len {
        let start = pos as usize;
        let end = (pos + step) as usize;

        let mut sum = 0.0;
        let mut count = 0;

        (start..end.min(input.len())).for_each(|i| {
            sum += input[i];
            count += 1;
        });

        out.push(if count > 0 { sum / count as f32 } else { 0.0 });
        pos += step;
    }

    out
}

#[allow(dead_code)]
fn write_wav_16k(path: &str, samples: &[f32]) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };

    if let Ok(mut writer) = hound::WavWriter::create(path, spec) {
        for &s in samples {
            let _ = writer.write_sample(s);
        }
        let _ = writer.finalize();
    }

    println!("AUDIO CAPTURED");
}

pub fn run_vad(
    mut audio: HeapCons<f32>,
    source_rate: u32,
    muted: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    mut on_utterance: impl FnMut(Vec<f32>),
) {
    let mut vad = Vad::new();
    vad.set_mode(webrtc_vad::VadMode::VeryAggressive);
    vad.set_sample_rate(webrtc_vad::SampleRate::Rate16kHz);

    const VAD_FRAME_16K: usize = 480; // 30 ms
    const MAX_SILENCE: usize = 50;

    let source_frame_size = ((source_rate / 100) * 3) as usize; // 30 ms @ source rate

    let mut resample_fifo = Vec::<f32>::new();
    let mut utterance = Vec::<f32>::new();

    let mut silence = 0;
    let mut speaking = false;
    let mut speaking_len = 0;

    loop {
        if shutdown.load(Ordering::Relaxed) {
            print!("\x1B[2J\x1B[1;1H");
            print!("Remembering conversation...");
            break;
        }


        if muted.load(Ordering::Relaxed) {
            if speaking {
                utterance.clear();
                resample_fifo.clear();
                speaking = false;
                speaking_len = 0;
                silence = 0;
            }

            if audio.occupied_len() >= source_frame_size {
                for _ in 0..source_frame_size {
                    audio.try_pop();
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(10));
            continue;
        }

        if audio.occupied_len() < source_frame_size {
            std::thread::sleep(std::time::Duration::from_millis(5));
            continue;
        }

        // Pull one source frame
        let mut frame = Vec::with_capacity(source_frame_size);
        for _ in 0..source_frame_size {
            frame.push(audio.try_pop().unwrap());
        }

        // Resample and accumulate
        let resampled = downsample_to_16k_box(&frame, source_rate);
        resample_fifo.extend(resampled);

        // Process fixed 16k frames
        while resample_fifo.len() >= VAD_FRAME_16K {
            let vad_frame: Vec<i16> = resample_fifo
                .drain(..VAD_FRAME_16K)
                .map(|x| (x.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                .collect();

            let speech = vad.is_voice_segment(&vad_frame).unwrap_or(false);

            if speech {
                speaking_len += 1;
                silence = 0;

                if speaking_len > 15 {
                    speaking = true;
                }

                utterance.extend(
                    vad_frame.iter().map(|&s| s as f32 / i16::MAX as f32)
                );
            } else if speaking {
                silence += 1;
                utterance.extend(
                    vad_frame.iter().map(|&s| s as f32 / i16::MAX as f32)
                );

                if silence >= MAX_SILENCE {
                    if utterance.len() >= 16_000 {
                        // write_wav_16k("utterance.wav", &utterance);
                        on_utterance(utterance.clone());
                    }

                    utterance.clear();
                    speaking = false;
                    speaking_len = 0;
                    silence = 0;
                }
            } else {
                speaking_len = 0;
            }
        }
    }
}
