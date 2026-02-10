use cpal::{
    Stream,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use ringbuf::{
    HeapRb,
    traits::{Producer, Split},
};

use crate::state::StateHandle;
use crate::ui;

pub fn start_mic(
    state: StateHandle,
) -> (ringbuf::HeapCons<f32>, std::mem::ManuallyDrop<Stream>, u32) {
    let host = cpal::default_host();

    let device = host.default_input_device().expect("No mic");

    let config = device.default_input_config().unwrap();
    let source_rate = (config.sample_rate() * config.channels() as u32).0;

    let rb = HeapRb::<f32>::new(48_000 * 10);
    let (mut producer, consumer) = rb.split();

    let stream = std::mem::ManuallyDrop::new(
        device
            .build_input_stream(
                &config.into(),
                move |data: &[f32], _| {
                    let current = state.read();
                    if !current.system_mute && !current.user_mute {
                        let _ = producer.push_slice(data);
                    }
                },
                |e| {
                    ui::error_stream(e);
                },
                None,
            )
            .unwrap(),
    );

    stream.play().unwrap();
    (consumer, stream, source_rate)
}
