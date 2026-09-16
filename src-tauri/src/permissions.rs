//! macOS privacy prompts. Calling AVCaptureDevice explicitly is what registers the app in
//! System Settings > Privacy & Security > Microphone and shows the user the allow dialog.

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2::{class, msg_send};
use objc2_foundation::NSString;

#[link(name = "AVFoundation", kind = "framework")]
extern "C" {}

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
