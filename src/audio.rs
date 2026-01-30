
use cpal::{Stream, traits::{DeviceTrait, HostTrait, StreamTrait}};
use ringbuf::{HeapRb, traits::{Producer, Split}};

use crate::ui;

pub fn start_mic() -> (ringbuf::HeapCons<f32>, std::mem::ManuallyDrop<Stream>, u32) {
    let host = cpal::default_host();

    let device =  host.default_input_device().expect("No mic");

    // if let Ok(inputs) = host.input_devices() {
    //     let devices: Vec<_> = inputs.filter(|d| d.supports_input() && d.default_input_config().is_ok()).collect();

    //     for (i, input) in devices.iter().enumerate() {
    //         if let Ok(description) = input.description() {
    //             println!("INPUT {i}: {}", description.name());
    //         }
    //     }

    //     let mut user_input = String::new();
    //     io::stdin()
    //         .read_line(&mut user_input)
    //         .expect("Failed to read line");

    //     let number: usize = user_input.trim().parse().expect("Failed to parse number");

    //     if number < devices.len() {
    //         device = devices[number].clone();
    //     }
    // }

    // if let Ok(description) = device.description() {
    //     println!("CHOSEN: {}", description.name());
    //     println!("DEVICE DATA: {:?}", device.default_input_config())
    // }

    // maybe in the future I'll make it request one channel from the device, but until then, have this hack
    let config = device.default_input_config().unwrap();
    let source_rate = config.sample_rate() * config.channels() as u32;

    let rb = HeapRb::<f32>::new(48_000 * 10);
    let (mut producer, consumer) = rb.split();

    let stream = std::mem::ManuallyDrop::new(device.build_input_stream(
        &config.into(),
        move |data: &[f32], _| {
            let _ = producer.push_slice(data);
        },
        |e| {
            ui::error_stream(e);
        },
        None,
    ).unwrap());

    stream.play().unwrap();
    (consumer, stream, source_rate)
}
