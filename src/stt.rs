use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, install_logging_hooks};

pub struct Stt {
    ctx: WhisperContext,
}

impl Stt {
    pub fn new(path: &str) -> anyhow::Result<Self> {
        install_logging_hooks();
        Ok(Self { ctx: WhisperContext::new_with_params(path, WhisperContextParameters {..Default::default()})? })
    }

    pub fn transcribe(&self, audio: &[f32]) -> anyhow::Result<String> {
        // let spec = hound::WavSpec {
        //     channels: 1,
        //     sample_rate: 16000,
        //     bits_per_sample: 32,
        //     sample_format: hound::SampleFormat::Float,
        // };
        // println!("{:?}", self.source_rate);

        // if let Ok(mut writer) = hound::WavWriter::create("utterance.wav", spec) {
        //     for &sample in resampled {
        //         let _ = writer.write_sample(sample);
        //     }
        //     let _ = writer.finalize();
        // }

        let mut state = self.ctx.create_state()?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 3 });
        params.set_language(Some("en"));
        params.set_n_threads(8);
        params.set_print_progress(false);
        params.set_print_special(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        state.full(params, audio)?;

        let mut out = String::new();
        for i in 0..state.full_n_segments() {
            if let Some(seg) = state.get_segment(i) {
                out.push_str(seg.to_str()?);
            }
        }

        Ok(out)
    }
}
