//! The other side of a call, for meeting transcripts (macOS 13+). ScreenCaptureKit captures audio
//! per app, so only the apps on the allow-list (call apps and browsers) are heard: Spotify, Music
//! or anything else playing at the same time never reaches the transcript. It needs the Screen
//! Recording permission even though no picture is kept.

use anyhow::{anyhow, Context, Result};
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_core_audio_types::{kAudioFormatFlagIsFloat, kAudioFormatFlagIsNonInterleaved};
use objc2_core_media::{CMAudioFormatDescriptionGetStreamBasicDescription, CMSampleBuffer, CMTime};
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol, NSOperatingSystemVersion, NSProcessInfo};
use objc2_screen_capture_kit::{
    SCContentFilter, SCRunningApplication, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamOutput,
    SCStreamOutputType,
};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::audio::{to_mono_16k, TARGET_RATE};

// Weakly linked (see build.rs) so the app still launches on macOS 12, where this framework is
// missing or cannot capture audio; `supported` keeps it from being touched there.
#[link(name = "ScreenCaptureKit", kind = "framework")]
extern "C" {}

/// How often the running apps are checked, so a call app opened after the meeting started is
/// heard from then on.
const REFRESH: Duration = Duration::from_secs(3);

/// ScreenCaptureKit captures app audio from macOS 13 on.
pub fn supported() -> bool {
    let v = NSOperatingSystemVersion { majorVersion: 13, minorVersion: 0, patchVersion: 0 };
    NSProcessInfo::processInfo().isOperatingSystemAtLeastVersion(v)
}

/// Whether a running app is on the allow-list. A listed id also covers its helper processes
/// (`com.google.Chrome` covers `com.google.Chrome.helper`), which is where browsers play sound.
pub fn allowed(bundle_id: &str, apps: &[String]) -> bool {
    apps.iter().any(|a| {
        bundle_id == a || (bundle_id.starts_with(a.as_str()) && bundle_id.as_bytes().get(a.len()) == Some(&b'.'))
    })
}

/// Live capture of the allowed apps' audio, appended to `sink` as 16 kHz mono. Stops on drop.
pub struct AppAudio {
    stream: Retained<SCStream>,
    _output: Retained<Output>,
    stop_refresh: Option<mpsc::Sender<()>>,
    refresh: Option<std::thread::JoinHandle<()>>,
}

// ScreenCaptureKit objects may be used from any thread; objc2 just cannot know that.
struct Shared<T>(T);
unsafe impl<T> Send for Shared<T> {}

impl AppAudio {
    pub fn start(apps: Vec<String>, sink: Arc<Mutex<Vec<f32>>>) -> Result<Self> {
        if !supported() {
            return Err(anyhow!("meeting transcripts need macOS 13 or newer"));
        }
        let content = shareable_content()?;
        let (filter, pids) = filter_for(&content.0, &apps)?;
        unsafe {
            let config = SCStreamConfiguration::new();
            config.setCapturesAudio(true);
            config.setSampleRate(TARGET_RATE as isize);
            config.setChannelCount(1);
            config.setExcludesCurrentProcessAudio(true);
            // The API always produces video too: ask for the smallest frames, once a second.
            config.setWidth(2);
            config.setHeight(2);
            config.setMinimumFrameInterval(CMTime::new(1, 1));

            let stream =
                SCStream::initWithFilter_configuration_delegate(SCStream::alloc(), &filter, &config, None);
            let output = Output::new(sink);
            let proto = ProtocolObject::from_ref(&*output);
            stream
                .addStreamOutput_type_sampleHandlerQueue_error(proto, SCStreamOutputType::Audio, None)
                .map_err(|e| anyhow!("adding the audio output: {}", e.localizedDescription()))?;
            // Video frames are dropped, but without a receiver ScreenCaptureKit logs every one.
            stream
                .addStreamOutput_type_sampleHandlerQueue_error(proto, SCStreamOutputType::Screen, None)
                .map_err(|e| anyhow!("adding the screen output: {}", e.localizedDescription()))?;
            wait(|done| stream.startCaptureWithCompletionHandler(Some(&done)))
                .context("starting system audio capture (is Screen Recording allowed for stfu?)")?;
            log::info!("meeting audio: capturing {}", describe(&pids));

            let (stop_tx, stop_rx) = mpsc::channel();
            let watched = Shared(stream.clone());
            let refresh = std::thread::Builder::new()
                .name("meeting-apps".into())
                .spawn(move || refresh_loop(watched, apps, pids, stop_rx))
                .context("spawning the app watcher")?;
            Ok(Self { stream, _output: output, stop_refresh: Some(stop_tx), refresh: Some(refresh) })
        }
    }
}

impl Drop for AppAudio {
    fn drop(&mut self) {
        drop(self.stop_refresh.take());
        if let Some(t) = self.refresh.take() {
            let _ = t.join();
        }
        if let Err(e) = wait(|done| unsafe { self.stream.stopCaptureWithCompletionHandler(Some(&done)) }) {
            log::warn!("stopping system audio capture: {e:#}");
        }
    }
}

/// Re-applies the filter whenever the set of running allowed apps changes.
fn refresh_loop(
    stream: Shared<Retained<SCStream>>,
    apps: Vec<String>,
    mut pids: Vec<(i32, String)>,
    stop: mpsc::Receiver<()>,
) {
    while let Err(mpsc::RecvTimeoutError::Timeout) = stop.recv_timeout(REFRESH) {
        let content = match shareable_content() {
            Ok(c) => c,
            Err(e) => {
                log::warn!("meeting audio: listing apps failed: {e:#}");
                continue;
            }
        };
        let (filter, now) = match filter_for(&content.0, &apps) {
            Ok(f) => f,
            Err(e) => {
                log::warn!("meeting audio: {e:#}");
                continue;
            }
        };
        if now == pids {
            continue;
        }
        match wait(|done| unsafe { stream.0.updateContentFilter_completionHandler(&filter, Some(&done)) }) {
            Ok(()) => {
                log::info!("meeting audio: now capturing {}", describe(&now));
                pids = now;
            }
            Err(e) => log::warn!("meeting audio: updating the app filter failed: {e:#}"),
        }
    }
}

/// A filter on the first display that hears only the allowed apps, plus their pids and names
/// (sorted, so two calls can be compared to see whether anything changed).
fn filter_for(
    content: &SCShareableContent,
    apps: &[String],
) -> Result<(Retained<SCContentFilter>, Vec<(i32, String)>)> {
    unsafe {
        let display = content
            .displays()
            .firstObject()
            .ok_or_else(|| anyhow!("no display to attach the audio capture to"))?;
        let chosen: Vec<Retained<SCRunningApplication>> = content
            .applications()
            .to_vec()
            .into_iter()
            .filter(|a| allowed(&a.bundleIdentifier().to_string(), apps))
            .collect();
        let mut pids: Vec<(i32, String)> =
            chosen.iter().map(|a| (a.processID(), a.applicationName().to_string())).collect();
        pids.sort();
        let filter = SCContentFilter::initWithDisplay_includingApplications_exceptingWindows(
            SCContentFilter::alloc(),
            &display,
            &NSArray::from_retained_slice(&chosen),
            &NSArray::new(),
        );
        Ok((filter, pids))
    }
}

fn describe(pids: &[(i32, String)]) -> String {
    if pids.is_empty() {
        return "nothing yet (no call app or browser is running)".into();
    }
    let mut names: Vec<&str> = pids.iter().map(|(_, n)| n.as_str()).collect();
    names.dedup();
    names.join(", ")
}

fn shareable_content() -> Result<Shared<Retained<SCShareableContent>>> {
    let (tx, rx) = mpsc::channel();
    let block = RcBlock::new(move |content: *mut SCShareableContent, err: *mut NSError| {
        let result = match unsafe { Retained::retain(content) } {
            Some(c) => Ok(Shared(c)),
            None => Err(error_text(err)),
        };
        let _ = tx.send(result);
    });
    unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&block) };
    rx.recv_timeout(Duration::from_secs(10))
        .map_err(|_| anyhow!("ScreenCaptureKit did not list the running apps"))?
        .map_err(|e| anyhow!("listing apps to capture: {e} (is Screen Recording allowed for stfu?)"))
}

/// Calls an API that reports completion through an error block, and waits for it.
fn wait(call: impl FnOnce(RcBlock<dyn Fn(*mut NSError)>)) -> Result<()> {
    let (tx, rx) = mpsc::channel();
    let block = RcBlock::new(move |err: *mut NSError| {
        let _ = tx.send(if err.is_null() { Ok(()) } else { Err(error_text(err)) });
    });
    call(block);
    rx.recv_timeout(Duration::from_secs(10))
        .map_err(|_| anyhow!("ScreenCaptureKit did not answer"))?
        .map_err(|e| anyhow!(e))
}

fn error_text(err: *mut NSError) -> String {
    unsafe { err.as_ref() }
        .map(|e| e.localizedDescription().to_string())
        .unwrap_or_else(|| "unknown ScreenCaptureKit error".into())
}

pub struct OutputIvars {
    sink: Arc<Mutex<Vec<f32>>>,
    warned: AtomicBool,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and Output does not implement Drop.
    #[unsafe(super(NSObject))]
    #[name = "StfuMeetingAudioOutput"]
    #[ivars = OutputIvars]
    pub struct Output;

    unsafe impl NSObjectProtocol for Output {}

    unsafe impl SCStreamOutput for Output {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn stream_did_output(&self, _stream: &SCStream, buffer: &CMSampleBuffer, kind: SCStreamOutputType) {
            if kind != SCStreamOutputType::Audio {
                return;
            }
            match unsafe { mono_16k(buffer) } {
                Ok(samples) => self.ivars().sink.lock().unwrap().extend_from_slice(&samples),
                Err(e) => {
                    if !self.ivars().warned.swap(true, Ordering::Relaxed) {
                        log::warn!("meeting audio: unreadable sample buffer: {e}");
                    }
                }
            }
        }
    }
);

impl Output {
    fn new(sink: Arc<Mutex<Vec<f32>>>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(OutputIvars { sink, warned: AtomicBool::new(false) });
        unsafe { msg_send![super(this), init] }
    }
}

/// The buffer's audio as 16 kHz mono. Asked for exactly that, but checked rather than assumed.
unsafe fn mono_16k(buffer: &CMSampleBuffer) -> Result<Vec<f32>, String> {
    let desc = buffer.format_description().ok_or("no format description")?;
    let asbd = CMAudioFormatDescriptionGetStreamBasicDescription(&desc)
        .as_ref()
        .ok_or("not an audio buffer")?;
    if asbd.mFormatFlags & kAudioFormatFlagIsFloat == 0 || asbd.mBitsPerChannel != 32 {
        return Err(format!("expected 32-bit float audio, got flags {:#x}", asbd.mFormatFlags));
    }
    let block = buffer.data_buffer().ok_or("no data")?;
    let mut samples = vec![0f32; block.data_length() / 4];
    if samples.is_empty() {
        return Ok(samples);
    }
    let dest = NonNull::new(samples.as_mut_ptr().cast()).ok_or("null buffer")?;
    let status = block.copy_data_bytes(0, samples.len() * 4, dest);
    if status != 0 {
        return Err(format!("copying audio failed: OSStatus {status}"));
    }
    let channels = asbd.mChannelsPerFrame.max(1) as u16;
    if channels > 1 && asbd.mFormatFlags & kAudioFormatFlagIsNonInterleaved != 0 {
        // Planar: one channel after the other. The first is enough for speech.
        samples.truncate(samples.len() / channels as usize);
        return Ok(to_mono_16k(&samples, asbd.mSampleRate as u32, 1));
    }
    Ok(to_mono_16k(&samples, asbd.mSampleRate as u32, channels))
}

#[cfg(test)]
mod tests {
    use super::allowed;

    fn list(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn listed_apps_and_their_helpers_are_heard() {
        let apps = list(&["us.zoom.xos", "com.google.Chrome"]);
        assert!(allowed("us.zoom.xos", &apps));
        assert!(allowed("com.google.Chrome", &apps));
        assert!(allowed("com.google.Chrome.helper", &apps));
        assert!(allowed("com.google.Chrome.helper.Renderer", &apps));
    }

    #[test]
    fn music_and_lookalike_ids_are_not() {
        let apps = list(&["us.zoom.xos", "com.google.Chrome"]);
        assert!(!allowed("com.spotify.client", &apps));
        assert!(!allowed("com.apple.Music", &apps));
        assert!(!allowed("com.google.ChromeCanary", &apps));
        assert!(!allowed("us.zoom", &apps));
    }
}
