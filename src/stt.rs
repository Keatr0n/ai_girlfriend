use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, install_logging_hooks};

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


pub struct Stt {
    ctx: WhisperContext,
    source_rate: u32,
}

impl Stt {
    pub fn new(path: &str, source_rate: u32) -> anyhow::Result<Self> {
        install_logging_hooks();
        Ok(Self { ctx: WhisperContext::new_with_params(path, WhisperContextParameters {..Default::default()})?, source_rate})
    }


    pub fn transcribe(&self, audio: &[f32]) -> anyhow::Result<String> {
        let resampled = &resample_to_16khz(audio, self.source_rate);

        let mut state = self.ctx.create_state()?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 3 });
        params.set_language(Some("en"));
        params.set_n_threads(8);
        params.set_print_progress(false);
        params.set_print_special(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        state.full(params, resampled)?;

        let mut out = String::new();
        for i in 0..state.full_n_segments() {
            if let Some(seg) = state.get_segment(i) {
                out.push_str(seg.to_str()?);
            }
        }

        Ok(out)
    }
}
