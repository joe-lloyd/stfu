//! macOS privacy prompts. Calling AVCaptureDevice explicitly is what registers the app in
//! System Settings > Privacy & Security > Microphone and shows the user the allow dialog.

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2::{class, msg_send};
use objc2_foundation::NSString;

#[link(name = "AVFoundation", kind = "framework")]
extern "C" {}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    /// 0 = granted, 1 = denied, 2 = not yet asked.
    fn IOHIDCheckAccess(request_type: u32) -> u32;
    fn IOHIDRequestAccess(request_type: u32) -> bool;
}

/// `kIOHIDRequestTypeListenEvent`: observing keystrokes, which is what the hotkey listener does.
const LISTEN_EVENT: u32 = 1;

/// Input Monitoring, the permission a global hotkey needs. Separate from Accessibility: without
/// it the event tap is created but macOS delivers nothing, so the hotkey silently does nothing.
pub fn input_monitoring_granted() -> bool {
    unsafe { IOHIDCheckAccess(LISTEN_EVENT) == 0 }
}

/// Shows the system's Input Monitoring prompt when it has never been asked. macOS only prompts
/// once ever, so a user who dismissed it must be sent to System Settings instead.
pub fn request_input_monitoring() -> bool {
    unsafe { IOHIDRequestAccess(LISTEN_EVENT) }
}

/// Microphone, without re-prompting.
pub fn microphone_granted() -> bool {
    let media = NSString::from_str("soun");
    let status: isize = unsafe { msg_send![class!(AVCaptureDevice), authorizationStatusForMediaType: &*media] };
    status == 3
}

use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::string::{CFString, CFStringRef};

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

/// Whether the app has Accessibility permission (System Settings > Privacy & Security).
pub fn accessibility_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// Like `accessibility_trusted`, but asks macOS to show its "allow Accessibility" dialog and
/// register the app in the Accessibility list when permission is missing.
pub fn request_accessibility() -> bool {
    unsafe {
        let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let opts = CFDictionary::from_CFType_pairs(&[(key, CFBoolean::true_value())]);
        AXIsProcessTrustedWithOptions(opts.as_concrete_TypeRef())
    }
}

/// AVAuthorizationStatus: 0 notDetermined, 1 restricted, 2 denied, 3 authorized.
pub fn request_microphone(on_result: impl Fn(bool) + Send + 'static) {
    let media = NSString::from_str("soun"); // AVMediaTypeAudio
    let cls = class!(AVCaptureDevice);
    let status: isize = unsafe { msg_send![cls, authorizationStatusForMediaType: &*media] };
    match status {
        3 => on_result(true),
        0 => {
            let block = RcBlock::new(move |granted: Bool| on_result(granted.as_bool()));
            let _: () = unsafe {
                msg_send![cls, requestAccessForMediaType: &*media, completionHandler: &*block]
            };
        }
        _ => on_result(false),
    }
}
