use std::ptr::NonNull;

use nice_plug_core::audio_setup::AudioIOLayout;
use vst3::Steinberg::Vst::{AudioBusBuffers, ProcessData, SymbolicSampleSizes_};

use crate::wrapper::util::buffer_management::{BufferManager, Buffers, ChannelPointers};

pub(super) const SAMPLE_32: i32 = SymbolicSampleSizes_::kSample32 as i32;
pub(super) const SAMPLE_64: i32 = SymbolicSampleSizes_::kSample64 as i32;

pub(super) enum Vst3BufferManager {
    Single(BufferManager<f32>),
    Double(BufferManager<f64>),
}

impl Vst3BufferManager {
    pub(super) fn new(double: bool, frames: usize, layout: AudioIOLayout) -> Self {
        if double {
            Self::Double(BufferManager::for_audio_io_layout(frames, layout))
        } else {
            Self::Single(BufferManager::for_audio_io_layout(frames, layout))
        }
    }
}

/// The host must supply valid bus arrays and audio for the requested interval. `channels`
/// must read the member of the VST3 union corresponding to the negotiated sample format.
pub(super) unsafe fn prepare<'a, 'b: 'a, T: Copy + Default + 'static>(
    manager: &'a mut BufferManager<T>,
    data: &ProcessData,
    start: usize,
    len: usize,
    layout: AudioIOLayout,
    channels: unsafe fn(&AudioBusBuffers) -> *mut *mut T,
) -> Buffers<'a, 'b, T> {
    let bus_pointers = |buses: *mut AudioBusBuffers, count: i32, index: usize| {
        if buses.is_null() || count <= index as i32 {
            return None;
        }
        // SAFETY(maybe): the bus index is within the array supplied by the host
        let bus = unsafe { &*buses.add(index) };
        if bus.numChannels <= 0 {
            return None;
        }
        NonNull::new(unsafe { channels(bus) }).map(|ptrs| ChannelPointers {
            ptrs,
            num_channels: bus.numChannels as usize,
        })
    };
    let main_input = usize::from(layout.main_input_channels.is_some());
    let main_output = usize::from(layout.main_output_channels.is_some());

    // SAFETY(maybe): the host owns the channels for the lifetime of this process call
    unsafe {
        manager.create_buffers(start, len, |source| {
            if main_input != 0 {
                *source.main_input_channel_pointers = bus_pointers(data.inputs, data.numInputs, 0);
            }
            if main_output != 0 {
                *source.main_output_channel_pointers =
                    bus_pointers(data.outputs, data.numOutputs, 0);
            }
            for (i, pointers) in source.aux_input_channel_pointers.iter_mut().enumerate() {
                *pointers = bus_pointers(data.inputs, data.numInputs, main_input + i);
            }
            for (i, pointers) in source.aux_output_channel_pointers.iter_mut().enumerate() {
                *pointers = bus_pointers(data.outputs, data.numOutputs, main_output + i);
            }
        })
    }
}

pub(super) fn valid<T>(buffers: &Buffers<'_, '_, T>) -> bool {
    buffers
        .main_buffer
        .as_slice_immutable()
        .iter()
        .chain(
            buffers
                .aux_outputs
                .iter()
                .flat_map(|buffer| buffer.as_slice_immutable()),
        )
        .all(|slice| !slice.is_empty())
}
