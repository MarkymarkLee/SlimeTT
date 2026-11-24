//! Microphone capture -> 16 kHz mono f32 chunks on a channel.
use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::mpsc::Sender;

pub const SR: u32 = 16000;

pub fn devices() -> Vec<String> {
    cpal::default_host()
        .input_devices()
        .map(|it| {
            it.filter_map(|d| d.description().ok().map(|d| d.name().to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// Open the input device (`""` = default) and stream resampled 16 kHz mono chunks to `tx`.
/// The returned Stream must be kept alive and dropped on the thread that created it.
pub fn start(device_name: &str, tx: Sender<Vec<f32>>) -> Result<cpal::Stream> {
    let host = cpal::default_host();
    // Opening a Bluetooth mic (AirPods) flips the headset into low-quality call mode for everything you hear.
    // So "System default" means: the default input, unless it's Bluetooth, then the built-in mic.
    let device_name = match (device_name.is_empty(), coreaudio::builtin_if_default_is_bluetooth()) {
        (true, Some(builtin)) => builtin,
        _ => device_name.to_string(),
    };
    let device_name = device_name.as_str();
    let device = if device_name.is_empty() {
        host.default_input_device()
    } else {
        host.input_devices()?
            .find(|d| d.description().map(|d| d.name() == device_name).unwrap_or(false))
    }
    .ok_or_else(|| anyhow!("input device not found: {device_name:?}"))?;
    let cfg = device.default_input_config().context("default input config")?;
    let (in_rate, channels) = (cfg.sample_rate(), cfg.channels() as usize);
    let scfg: cpal::StreamConfig = cfg.into();
    let mut resampler = Resampler::new(in_rate, SR);
    let stream = device
        .build_input_stream(
            scfg,
            move |data: &[f32], _| {
                let mono: Vec<f32> = data
                    .chunks(channels)
                    .map(|f| f.iter().sum::<f32>() / channels as f32)
                    .collect();
                let out = resampler.process(&mono);
                if !out.is_empty() {
                    tx.send(out).ok();
                }
            },
            |e| log::error!("audio stream: {e}"),
            None,
        )
        .context("build input stream")?;
    stream.play()?;
    Ok(stream)
}

/// Just enough CoreAudio to read device transport types (cpal reports them as Unknown on macOS).
mod coreaudio {
    use std::ffi::c_void;
    #[repr(C)]
    struct Addr {
        selector: u32,
        scope: u32,
        element: u32,
    }
    #[link(name = "CoreAudio", kind = "framework")]
    unsafe extern "C" {
        fn AudioObjectGetPropertyDataSize(id: u32, a: *const Addr, qs: u32, q: *const c_void, size: *mut u32) -> i32;
        fn AudioObjectGetPropertyData(
            id: u32,
            a: *const Addr,
            qs: u32,
            q: *const c_void,
            size: *mut u32,
            data: *mut c_void,
        ) -> i32;
    }
    const fn fourcc(s: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*s)
    }
    const SYSTEM: u32 = 1;
    const GLOBAL: u32 = fourcc(b"glob");
    const INPUT: u32 = fourcc(b"inpt");
    fn get<T: Copy + Default>(id: u32, selector: u32, scope: u32) -> Option<T> {
        let (a, mut v, mut size) = (
            Addr {
                selector,
                scope,
                element: 0,
            },
            T::default(),
            std::mem::size_of::<T>() as u32,
        );
        (unsafe { AudioObjectGetPropertyData(id, &a, 0, std::ptr::null(), &mut size, (&mut v as *mut T).cast()) } == 0)
            .then_some(v)
    }
    fn transport(id: u32) -> u32 {
        get(id, fourcc(b"tran"), GLOBAL).unwrap_or(0)
    }
    fn is_bluetooth(id: u32) -> bool {
        matches!(transport(id), t if t == fourcc(b"blue") || t == fourcc(b"blea"))
    }
    fn name(id: u32) -> Option<String> {
        let cf: usize = get(id, fourcc(b"lnam"), GLOBAL)?; // CFStringRef, +1 retained
        let cf = std::ptr::NonNull::new(cf as *mut objc2_core_foundation::CFString)?;
        Some(unsafe { objc2_core_foundation::CFRetained::from_raw(cf) }.to_string())
    }
    fn input_devices() -> Vec<u32> {
        let a = Addr {
            selector: fourcc(b"dev#"),
            scope: GLOBAL,
            element: 0,
        };
        let mut size = 0u32;
        if unsafe { AudioObjectGetPropertyDataSize(SYSTEM, &a, 0, std::ptr::null(), &mut size) } != 0 {
            return Vec::new();
        }
        let mut ids = vec![0u32; size as usize / 4];
        if unsafe { AudioObjectGetPropertyData(SYSTEM, &a, 0, std::ptr::null(), &mut size, ids.as_mut_ptr().cast()) }
            != 0
        {
            return Vec::new();
        }
        // keep devices with at least one input stream
        ids.into_iter()
            .filter(|&id| {
                let a = Addr {
                    selector: fourcc(b"stm#"),
                    scope: INPUT,
                    element: 0,
                };
                let mut size = 0u32;
                (unsafe { AudioObjectGetPropertyDataSize(id, &a, 0, std::ptr::null(), &mut size) }) == 0 && size > 0
            })
            .collect()
    }
    /// Name of the built-in mic when the system default input is a Bluetooth device, else None.
    pub fn builtin_if_default_is_bluetooth() -> Option<String> {
        let default: u32 = get(SYSTEM, fourcc(b"dIn "), GLOBAL)?;
        if !is_bluetooth(default) {
            return None;
        }
        input_devices()
            .into_iter()
            .find(|&id| transport(id) == fourcc(b"bltn"))
            .and_then(name)
    }
    #[cfg(test)]
    pub fn describe() -> Vec<(String, String)> {
        input_devices()
            .into_iter()
            .map(|id| {
                (
                    name(id).unwrap_or_default(),
                    String::from_utf8_lossy(&transport(id).to_be_bytes()).into(),
                )
            })
            .collect()
    }
}

/// Linear-interpolation resampler, good enough for speech going into a 16 kHz model.
struct Resampler {
    step: f64,
    pos: f64,
    last: f32,
}
impl Resampler {
    fn new(from: u32, to: u32) -> Self {
        Self {
            step: from as f64 / to as f64,
            pos: 0.0,
            last: 0.0,
        }
    }
    fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if self.step == 1.0 {
            return input.to_vec();
        }
        let mut out = Vec::with_capacity((input.len() as f64 / self.step) as usize + 1);
        // virtual index -1 is `last` (final sample of the previous chunk)
        while self.pos < input.len() as f64 {
            let i = self.pos.floor();
            let frac = (self.pos - i) as f32;
            let a = if i < 0.0 { self.last } else { input[i as usize] };
            let b = if (i as usize + 1) < input.len() {
                input[i as usize + 1]
            } else {
                input[input.len() - 1]
            };
            out.push(a + (b - a) * frac);
            self.pos += self.step;
        }
        self.pos -= input.len() as f64;
        self.last = input[input.len() - 1];
        out
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn coreaudio_transport_types() {
        let devs = super::coreaudio::describe();
        println!(
            "{devs:?} -> bluetooth default replaced by {:?}",
            super::coreaudio::builtin_if_default_is_bluetooth()
        );
        assert!(!devs.is_empty(), "at least one input device");
    }
    use super::*;
    #[test]
    fn resample_ratio_holds_across_chunks() {
        let mut r = Resampler::new(48000, 16000);
        let total: usize = (0..10).map(|_| r.process(&vec![0.5; 480]).len()).sum();
        assert_eq!(total, 1600);
        assert_eq!(Resampler::new(16000, 16000).process(&[1.0, 2.0]).len(), 2);
    }
}
