#![allow(incomplete_features)]

#![feature(const_generics)]
#![feature(slice_fill)]


use std::sync::Arc;
use std::mem;
use std::marker::PhantomData;
use vst::buffer::AudioBuffer;
use vst::plugin::{Category, Info, Plugin, PluginParameters};
use vst::plugin_main;
use vst::util::AtomicFloat;

const CHANNELS: usize = 2;

#[derive(Default)]
struct SnapshotRepeatPlugin {
    params: Arc<Params>,
    channel_states: [ChannelState; CHANNELS],
}

trait Scale {
    fn to_norm(scaled: f32) -> f32;
    fn to_scaled(norm: f32) -> f32;
}

struct LinScale<const LOW: f32, const HIGH: f32>;

impl<const LOW: f32, const HIGH: f32> Scale for LinScale<LOW, HIGH> {
    fn to_norm(scaled: f32) -> f32 {
        debug_assert!(LOW <= scaled && scaled <= HIGH);
        (scaled - LOW) / (HIGH - LOW)
    }

    fn to_scaled(norm: f32) -> f32 {
        debug_assert!(0.0 <= norm && norm <= 1.0);
        LOW + norm * (HIGH - LOW)
    }
}

struct ScaledParameter<S: Scale> {
    inner: AtomicFloat,
    _scale: PhantomData<S>,
}

impl<S: Scale> ScaledParameter<S> {
    fn new(init: f32) -> Self {
        Self {
            inner: AtomicFloat::new(S::to_norm(init)),
            _scale: PhantomData,
        }
    }

    fn get_raw(&self) -> f32 {
        self.inner.get()
    }

    fn set_raw(&self, val: f32) {
        self.inner.set(val)
    }

    fn get(&self) -> f32 {
        S::to_scaled(self.get_raw())
    }
}

struct Params {
    /// period between recapturing
    period: ScaledParameter<LinScale<1.0, { 44_100.0 * 10.0 }>>,
    /// size of the captured buffer
    capture_len: ScaledParameter<LinScale<1.0, { 44_100.0 * 10.0 }>>,
    /// playback rate
    playback_rate: ScaledParameter<LinScale<0.01, 100.0>>,
}

impl Default for Params {
    fn default() -> Params {
        Params {
            period: ScaledParameter::new(44_100.0),
            capture_len: ScaledParameter::new(44_100.0),
            playback_rate: ScaledParameter::new(1.0),
        }
    }
}

struct ChannelState {
    // buffer used for interpolation
    current_buffer: Box<[f32]>,
    // normalized (0.0 .. 1.0) offset into the current buffer
    current_offset_norm: f32,

    // how many samples used the current buffer
    current_offset_total: usize,
    // how many samples should the current buffer be used for in total
    current_period: usize,

    // the buffer to be used next (if any)
    next_buffer: Box<[f32]>,
    // how many of the samples in the next buffer have been written
    next_buffer_len: usize,
}

impl Default for ChannelState {
    fn default() -> Self {
        Self {
            current_buffer: Box::new([]),
            current_offset_norm: 0.0,
            current_offset_total: 0,
            current_period: 0,
            next_buffer: Box::new([]),
            next_buffer_len: 0,
        }
    }
}

impl Plugin for SnapshotRepeatPlugin {
    fn get_info(&self) -> Info {
        Info {
            name: "Snapshot Repeat".to_string(),
            vendor: "ametisf".to_string(),
            unique_id: 141375252,
            version: 1,
            inputs: CHANNELS as i32,
            outputs: CHANNELS as i32,
            parameters: 3,
            category: Category::Effect,
            ..Default::default()
        }
    }

    fn get_parameter_object(&mut self) -> Arc<dyn PluginParameters> {
        Arc::clone(&self.params) as _
    }

    fn process(&mut self, buffer: &mut AudioBuffer<f32>) {
        debug_assert!(
            buffer.input_count() == CHANNELS &&
            buffer.output_count() == CHANNELS
        );

        let params = &*self.params;
        buffer.zip()
            .zip(&mut self.channel_states)
            .for_each(|((input_buffer, output_buffer), chan_state)| {
                process_channel(params, chan_state, input_buffer, output_buffer)
            });
    }
}

// all the actual DSP logic is here
fn process_channel(
    params: &Params,
    state: &mut ChannelState,
    inp: &[f32],
    out: &mut [f32],
) {
    let period = params.period.get().round() as usize;
    // dbg!(period);
    let capture_len = params.capture_len.get().round() as usize;
    // dbg!(capture_len);
    let playback_rate = params.playback_rate.get();
    // dbg!(playback_rate);
    // eprintln!("");

    // finished one period, swap buffers and update parameters
    if state.current_offset_total >= state.current_period {
        state.current_period = period;
        state.current_offset_total = 0;

        // takes the minimum because we can't manage to capture more than `period` samples
        let next_buffer_size = usize::min(capture_len, period);
        state.next_buffer_len = 0;

        state.current_buffer = mem::replace(
            &mut state.next_buffer,
            vec![0.0; next_buffer_size].into_boxed_slice(),
        );
        state.current_offset_norm = 0.0;
    }
    state.current_offset_total += inp.len();

    // if the next buffer is not full write to it from the input
    if state.next_buffer.len() > state.next_buffer_len {
        inp.iter().zip(&mut state.next_buffer[state.next_buffer_len..])
            .for_each(|(inp, out)| *out = *inp);
        state.next_buffer_len += inp.len();
    }

    // keep quiet if the buffer is empty
    if state.current_buffer.len() == 0 {
        out.fill(0.0);
        return
    }

    // use the last recorded buffer as a wavetable, scan at the original speed * playback_rate
    let mut offset = state.current_offset_norm;
    let increment = (1.0 / state.current_buffer.len() as f32) * playback_rate;
    let buffer = &state.current_buffer;
    for out in out {
        let idx = offset * (buffer.len() as f32);
        let low_idx = idx.floor() as usize;
        let high_idx = (low_idx + 1) % buffer.len();
        let fract = idx.fract();

        let low = buffer[low_idx];
        let high = buffer[high_idx];

        *out = low + (high - low) * fract;

        *out = buffer[low_idx];
        offset = (offset + increment) % 1.0;
    }
    state.current_offset_norm = offset;
}

impl PluginParameters for Params {
    fn get_parameter(&self, index: i32) -> f32 {
        match index {
            0 => self.period.get_raw(),
            1 => self.capture_len.get_raw(),
            2 => self.playback_rate.get_raw(),
            _ => 0.0,
        }
    }

    fn set_parameter(&self, index: i32, val: f32) {
        match index {
            0 => self.period.set_raw(val),
            1 => self.capture_len.set_raw(val),
            2 => self.playback_rate.set_raw(val),
            _ => {}
        }
    }

    fn get_parameter_text(&self, index: i32) -> String {
        match index {
            0 => format!("{:.2} samples", self.period.get()),
            1 => format!("{:.2} samples", self.capture_len.get()),
            2 => format!("{:.2}x", self.playback_rate.get()),
            _ => "".to_string(),
        }
    }

    fn get_parameter_name(&self, index: i32) -> String {
        match index {
            0 => "Period",
            1 => "Capture length",
            2 => "Playback rate",
            _ => "",
        }
        .to_string()
    }
}

plugin_main!(SnapshotRepeatPlugin);
