//! System audio through a Core Audio process tap. A tap needs only permission
//! to record system audio, which is a smaller ask than the Screen Recording
//! permission ScreenCaptureKit needs, and it can leave Tidy's own sounds out.
//!
//! The tap is wrapped in a private aggregate device holding only the tap, as
//! adding the output device as well doubles the audio.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::AllocAnyThread;
use objc2_core_audio::{
    kAudioAggregateDeviceIsPrivateKey, kAudioAggregateDeviceNameKey, kAudioAggregateDeviceTapAutoStartKey,
    kAudioAggregateDeviceTapListKey, kAudioAggregateDeviceUIDKey, kAudioHardwarePropertyTranslatePIDToProcessObject,
    kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, kAudioSubTapUIDKey,
    kAudioTapPropertyFormat, AudioDeviceCreateIOProcIDWithBlock, AudioDeviceDestroyIOProcID, AudioDeviceIOProcID,
    AudioDeviceStart, AudioDeviceStop, AudioHardwareCreateAggregateDevice, AudioHardwareCreateProcessTap,
    AudioHardwareDestroyAggregateDevice, AudioHardwareDestroyProcessTap, AudioObjectGetPropertyData, AudioObjectID,
    AudioObjectPropertyAddress, CATapDescription,
};
use objc2_core_audio_types::{AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp};
use objc2_core_foundation::CFDictionary;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};

use crate::error::{AppError, AppResult};

type IoBlock = RcBlock<
    dyn Fn(
        NonNull<AudioTimeStamp>,
        NonNull<AudioBufferList>,
        NonNull<AudioTimeStamp>,
        NonNull<AudioBufferList>,
        NonNull<AudioTimeStamp>,
    ),
>;

/// A running tap. Dropping it stops capture and removes the tap and device.
pub struct TapCapture {
    tap: AudioObjectID,
    device: AudioObjectID,
    proc_id: AudioDeviceIOProcID,
    _block: Option<IoBlock>,
    pub sample_rate: u32,
}

impl Drop for TapCapture {
    fn drop(&mut self) {
        // SAFETY: each id was created below and is destroyed exactly once.
        unsafe {
            if self.proc_id.is_some() {
                AudioDeviceStop(self.device, self.proc_id);
                AudioDeviceDestroyIOProcID(self.device, self.proc_id);
            }
            if self.device != 0 {
                AudioHardwareDestroyAggregateDevice(self.device);
            }
            if self.tap != 0 {
                AudioHardwareDestroyProcessTap(self.tap);
            }
        }
    }
}

fn check(status: i32, what: &str) -> AppResult<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(AppError::Other(format!("Could not {what} (Core Audio error {status}).")))
    }
}

fn global(selector: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    }
}

/// Tidy's own audio process, so the tap can leave its sounds out. Tidy has
/// no audio process until it has used audio, in which case there is nothing
/// to leave out.
fn own_process() -> Option<AudioObjectID> {
    let pid = std::process::id() as i32;
    let mut address = global(kAudioHardwarePropertyTranslatePIDToProcessObject);
    let mut object: AudioObjectID = 0;
    let mut size = std::mem::size_of::<AudioObjectID>() as u32;
    // SAFETY: the qualifier is a pid_t and the output is one AudioObjectID.
    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject as AudioObjectID,
            NonNull::from(&mut address),
            std::mem::size_of::<i32>() as u32,
            (&pid as *const i32).cast(),
            NonNull::from(&mut size),
            NonNull::from(&mut object).cast(),
        )
    };
    (status == 0 && object != 0).then_some(object)
}

fn tap_format(tap: AudioObjectID) -> AppResult<AudioStreamBasicDescription> {
    let mut address = global(kAudioTapPropertyFormat);
    // SAFETY: a plain C struct of numbers, valid when zeroed.
    let mut format: AudioStreamBasicDescription = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of::<AudioStreamBasicDescription>() as u32;
    // SAFETY: the output buffer is one AudioStreamBasicDescription.
    let status = unsafe {
        AudioObjectGetPropertyData(
            tap,
            NonNull::from(&mut address),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut format).cast(),
        )
    };
    check(status, "read the system audio format")?;
    Ok(format)
}

fn key(name: &std::ffi::CStr) -> Retained<NSString> {
    NSString::from_str(name.to_str().unwrap_or_default())
}

/// Copy one IO cycle's audio into `buf` as mono samples. Taps deliver 32-bit
/// float, either one interleaved buffer or one buffer per channel.
///
/// # Safety
/// `list` must point to a valid AudioBufferList for the duration of the call.
unsafe fn copy_mono(list: &AudioBufferList, buf: &Mutex<Vec<f32>>) {
    let count = list.mNumberBuffers as usize;
    if count == 0 {
        return;
    }
    // SAFETY: an AudioBufferList holds `mNumberBuffers` buffers inline.
    let buffers: &[AudioBuffer] = unsafe { std::slice::from_raw_parts(list.mBuffers.as_ptr(), count) };
    let planes: Vec<(&[f32], usize)> = buffers
        .iter()
        .filter(|b| !b.mData.is_null())
        .map(|b| {
            let samples = b.mDataByteSize as usize / std::mem::size_of::<f32>();
            // SAFETY: Core Audio hands us `mDataByteSize` bytes of float audio.
            let data = unsafe { std::slice::from_raw_parts(b.mData.cast::<f32>(), samples) };
            (data, (b.mNumberChannels as usize).max(1))
        })
        .collect();
    let mono: Vec<f32> = match planes.as_slice() {
        [] => return,
        [(data, 1)] => data.to_vec(),
        [(data, channels)] => super::pipeline::downmix_to_mono(data, *channels),
        many => {
            let frames = many.iter().map(|(d, c)| d.len() / c).min().unwrap_or(0);
            (0..frames)
                .map(|i| many.iter().map(|(d, c)| d[i * c]).sum::<f32>() / many.len() as f32)
                .collect()
        }
    };
    if let Ok(mut b) = buf.lock() {
        b.extend_from_slice(&mono);
    }
}

/// Start capturing everything the Mac plays except Tidy itself, appending
/// mono samples at `sample_rate` to `buf`.
pub fn start_tap(buf: Arc<Mutex<Vec<f32>>>) -> AppResult<TapCapture> {
    let excluded: Vec<Retained<NSNumber>> =
        own_process().map(NSNumber::numberWithUnsignedInt).into_iter().collect();
    let excluded = NSArray::from_retained_slice(&excluded);
    // SAFETY: plain Objective-C object construction and property setters.
    let description = unsafe {
        let d = CATapDescription::initMonoGlobalTapButExcludeProcesses(CATapDescription::alloc(), &excluded);
        d.setPrivate(true);
        d.setName(&NSString::from_str("Tidy meeting audio"));
        d
    };
    let mut capture = TapCapture { tap: 0, device: 0, proc_id: None, _block: None, sample_rate: 48_000 };
    // SAFETY: the description is valid and the output is one AudioObjectID.
    check(
        unsafe { AudioHardwareCreateProcessTap(Some(&description), &mut capture.tap) },
        "create the system audio tap",
    )?;
    let format = tap_format(capture.tap)?;
    if format.mSampleRate < 8_000.0 {
        return Err(AppError::Other("The system audio tap reported no sample rate.".into()));
    }
    capture.sample_rate = format.mSampleRate.round() as u32;

    // SAFETY: reading the tap's UUID string from a valid description.
    let tap_uid = unsafe { description.UUID().UUIDString() };
    let device_uid = NSString::from_str(&format!("com.tidy.meeting-audio.{tap_uid}"));
    let sub_tap = NSDictionary::from_slices(&[&*key(kAudioSubTapUIDKey)], &[AsRef::<AnyObject>::as_ref(&*tap_uid)]);
    let taps = NSArray::from_retained_slice(&[sub_tap]);
    let name = NSString::from_str("Tidy meeting audio");
    let yes = NSNumber::numberWithBool(true);
    let keys = [
        key(kAudioAggregateDeviceNameKey),
        key(kAudioAggregateDeviceUIDKey),
        key(kAudioAggregateDeviceIsPrivateKey),
        key(kAudioAggregateDeviceTapAutoStartKey),
        key(kAudioAggregateDeviceTapListKey),
    ];
    let values: [&AnyObject; 5] = [name.as_ref(), device_uid.as_ref(), yes.as_ref(), yes.as_ref(), taps.as_ref()];
    let key_refs: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
    let settings = NSDictionary::from_slices(&key_refs, &values);
    // SAFETY: NSDictionary is toll-free bridged with CFDictionary.
    let settings_cf: &CFDictionary = unsafe { &*(Retained::as_ptr(&settings) as *const CFDictionary) };
    check(
        unsafe { AudioHardwareCreateAggregateDevice(settings_cf, NonNull::from(&mut capture.device)) },
        "create the system audio device",
    )?;

    let sink = buf.clone();
    let block: IoBlock = RcBlock::new(
        move |_now: NonNull<AudioTimeStamp>,
              input: NonNull<AudioBufferList>,
              _input_time: NonNull<AudioTimeStamp>,
              _output: NonNull<AudioBufferList>,
              _output_time: NonNull<AudioTimeStamp>| {
            // SAFETY: Core Audio passes a valid input buffer list for this cycle.
            unsafe { copy_mono(input.as_ref(), &sink) };
        },
    );
    let mut proc_id: AudioDeviceIOProcID = None;
    // SAFETY: the block lives as long as `capture`, which stops the device
    // before releasing it.
    check(
        unsafe {
            AudioDeviceCreateIOProcIDWithBlock(
                NonNull::from(&mut proc_id),
                capture.device,
                None,
                RcBlock::as_ptr(&block),
            )
        },
        "listen to system audio",
    )?;
    capture.proc_id = proc_id;
    capture._block = Some(block);
    check(unsafe { AudioDeviceStart(capture.device, capture.proc_id) }, "start system audio")?;
    Ok(capture)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(buffers: &mut [Vec<f32>], channels: u32) -> Vec<u8> {
        // An AudioBufferList with room for every buffer, laid out as Core Audio does.
        let size = std::mem::size_of::<AudioBufferList>() + (buffers.len().saturating_sub(1)) * std::mem::size_of::<AudioBuffer>();
        let mut bytes = vec![0u8; size];
        let list = bytes.as_mut_ptr().cast::<AudioBufferList>();
        unsafe {
            (*list).mNumberBuffers = buffers.len() as u32;
            let first = (*list).mBuffers.as_mut_ptr();
            for (i, b) in buffers.iter_mut().enumerate() {
                *first.add(i) = AudioBuffer {
                    mNumberChannels: channels,
                    mDataByteSize: (b.len() * 4) as u32,
                    mData: b.as_mut_ptr().cast::<c_void>(),
                };
            }
        }
        bytes
    }

    #[test]
    fn tap_audio_arrives_as_mono_whatever_its_layout() {
        let sink = Mutex::new(Vec::new());
        let mut mono = [vec![0.1, 0.2, 0.3]];
        let bytes = list(&mut mono, 1);
        unsafe { copy_mono(&*bytes.as_ptr().cast(), &sink) };
        assert_eq!(*sink.lock().unwrap(), vec![0.1, 0.2, 0.3]);

        let sink = Mutex::new(Vec::new());
        let mut interleaved = [vec![1.0, 0.0, 0.5, 0.5]];
        let bytes = list(&mut interleaved, 2);
        unsafe { copy_mono(&*bytes.as_ptr().cast(), &sink) };
        assert_eq!(*sink.lock().unwrap(), vec![0.5, 0.5]);

        let sink = Mutex::new(Vec::new());
        let mut planar = [vec![1.0, 0.5], vec![0.0, 0.5]];
        let bytes = list(&mut planar, 1);
        unsafe { copy_mono(&*bytes.as_ptr().cast(), &sink) };
        assert_eq!(*sink.lock().unwrap(), vec![0.5, 0.5]);
    }
}
