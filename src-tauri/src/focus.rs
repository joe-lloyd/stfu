//! Is the keyboard focus in a text field? Decides whether a dictation also stays on the
//! clipboard: with no text field to paste into, the text would otherwise be lost.
//!
//! macOS answers through the Accessibility API, which the app already needs for pasting. Native
//! apps answer reliably. Chromium and Electron apps (Chrome, Slack, VS Code, Teams, ...) keep
//! their accessibility tree off unless a screen reader asks, and then report no focused element
//! at all, so for them the answer is "unknown". Elsewhere it is always unknown for now.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    TextField,
    NoTextField,
    Unknown,
}

impl Focus {
    pub fn as_str(self) -> &'static str {
        match self {
            Focus::TextField => "text_field",
            Focus::NoTextField => "no_text_field",
            Focus::Unknown => "unknown",
        }
    }
}

/// Roles that always take typed text.
const TEXT_ROLES: [&str; 4] = ["AXTextField", "AXTextArea", "AXComboBox", "AXSearchField"];

/// Decides from what the focused element reports. Kept apart from the API calls so it can be
/// tested. `role` is None when the app reported no focused element or did not answer.
pub fn classify(role: Option<&str>, value_settable: bool, has_text_range: bool, editable_ancestor: bool) -> Focus {
    let Some(role) = role else { return Focus::Unknown };
    if TEXT_ROLES.contains(&role) || (value_settable && has_text_range) || editable_ancestor {
        return Focus::TextField;
    }
    // A whole web page has focus. It may be an editor that is the page itself (Mail's compose
    // window, Google Docs), so this says nothing either way.
    if role == "AXWebArea" {
        return Focus::Unknown;
    }
    Focus::NoTextField
}

#[cfg(target_os = "macos")]
pub fn current() -> Focus {
    use core_foundation::base::{CFGetTypeID, CFRelease, CFTypeRef, TCFType};
    use core_foundation::string::{CFString, CFStringGetTypeID, CFStringRef};
    use std::ffi::c_void;

    type Element = *const c_void;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> Element;
        fn AXUIElementCopyAttributeValue(element: Element, attribute: CFStringRef, value: *mut CFTypeRef) -> i32;
        fn AXUIElementIsAttributeSettable(element: Element, attribute: CFStringRef, settable: *mut u8) -> i32;
        fn AXUIElementSetMessagingTimeout(element: Element, seconds: f32) -> i32;
    }

    /// Owns one retained CF object and releases it when dropped.
    struct Owned(CFTypeRef);
    impl Drop for Owned {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CFRelease(self.0) }
            }
        }
    }

    fn copy(element: Element, attribute: &str) -> Option<Owned> {
        let name = CFString::new(attribute);
        let mut value: CFTypeRef = std::ptr::null();
        let err = unsafe { AXUIElementCopyAttributeValue(element, name.as_concrete_TypeRef(), &mut value) };
        (err == 0 && !value.is_null()).then_some(Owned(value))
    }

    let Some(pid) = frontmost_pid() else { return Focus::Unknown };
    let app = Owned(unsafe { AXUIElementCreateApplication(pid) });
    if app.0.is_null() {
        return Focus::Unknown;
    }
    // A hung app must not hold up the paste: give up quickly and call it unknown.
    unsafe { AXUIElementSetMessagingTimeout(app.0, 0.25) };
    let Some(focused) = copy(app.0, "AXFocusedUIElement") else { return Focus::Unknown };
    let element = focused.0;

    let role = copy(element, "AXRole").and_then(|r| unsafe {
        (CFGetTypeID(r.0) == CFStringGetTypeID())
            .then(|| CFString::wrap_under_get_rule(r.0 as CFStringRef).to_string())
    });
    let value_settable = {
        let name = CFString::new("AXValue");
        let mut settable = 0u8;
        let err = unsafe { AXUIElementIsAttributeSettable(element, name.as_concrete_TypeRef(), &mut settable) };
        err == 0 && settable != 0
    };
    let has_text_range = copy(element, "AXSelectedTextRange").is_some();
    // WebKit marks everything inside an editable region (a contenteditable editor) this way.
    let editable_ancestor = copy(element, "AXEditableAncestor").is_some();
    classify(role.as_deref(), value_settable, has_text_range, editable_ancestor)
}

#[cfg(target_os = "macos")]
fn frontmost_pid() -> Option<i32> {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

    #[link(name = "AppKit", kind = "framework")]
    extern "C" {}

    objc2::rc::autoreleasepool(|_| unsafe {
        let ws: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        if ws.is_null() {
            return None;
        }
        let app: *mut AnyObject = msg_send![&*ws, frontmostApplication];
        if app.is_null() {
            return None;
        }
        let pid: i32 = msg_send![&*app, processIdentifier];
        (pid > 0).then_some(pid)
    })
}

#[cfg(not(target_os = "macos"))]
pub fn current() -> Focus {
    Focus::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_roles_and_editable_elements_are_text_fields() {
        assert_eq!(classify(Some("AXTextArea"), true, true, false), Focus::TextField);
        assert_eq!(classify(Some("AXTextField"), false, false, false), Focus::TextField);
        assert_eq!(classify(Some("AXGroup"), true, true, false), Focus::TextField);
        assert_eq!(classify(Some("AXGroup"), false, false, true), Focus::TextField);
    }

    #[test]
    fn other_controls_are_not_but_silence_and_web_pages_are_unknown() {
        // Finder's file list reports a text range but cannot be typed into.
        assert_eq!(classify(Some("AXGroup"), false, true, false), Focus::NoTextField);
        assert_eq!(classify(Some("AXOutline"), false, false, false), Focus::NoTextField);
        assert_eq!(classify(Some("AXSlider"), true, false, false), Focus::NoTextField);
        assert_eq!(classify(Some("AXWebArea"), false, true, false), Focus::Unknown);
        assert_eq!(classify(None, false, false, false), Focus::Unknown);
    }

    #[test]
    fn asking_never_fails_or_hangs() {
        // Whatever is in front while tests run; the point is that it answers quickly.
        let t = std::time::Instant::now();
        let _ = current();
        assert!(t.elapsed() < std::time::Duration::from_secs(2));
    }
}
