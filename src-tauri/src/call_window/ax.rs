//! The macOS Accessibility calls used to read a call window: the same
//! information VoiceOver reads. Tidy only reads, apart from the switches that
//! Chromium-based apps (Chrome, Edge and Teams itself) need before they share
//! a web page's elements, which are put back when the recording ends.
//!
//! Every call into another app waits for that app to answer, so each read
//! has a short timeout, a time budget and a limit on how many elements it
//! visits. An app that cannot answer ends the read straight away.

use std::ffi::{c_char, c_int, c_void};
use std::ptr::null;
use std::time::{Duration, Instant};

use super::scan::Node;

type CFTypeRef = *const c_void;
type CFIndex = isize;
type CFTypeID = usize;
type AXError = i32;

const UTF8: u32 = 0x0800_0100;
const AX_SUCCESS: AXError = 0;
const AX_CANNOT_COMPLETE: AXError = -25204;
/// How long one call may wait for the other app, in seconds.
const CALL_TIMEOUT: f32 = 0.5;
/// Deeper than this is never a call's tiles or captions.
const MAX_DEPTH: usize = 60;

#[repr(C)]
struct CFArrayCallBacks {
    version: CFIndex,
    retain: *const c_void,
    release: *const c_void,
    copy_description: *const c_void,
    equal: *const c_void,
}

#[repr(C)]
struct CFDictionaryKeyCallBacks {
    version: CFIndex,
    retain: *const c_void,
    release: *const c_void,
    copy_description: *const c_void,
    equal: *const c_void,
    hash: *const c_void,
}

#[repr(C)]
struct CFDictionaryValueCallBacks {
    version: CFIndex,
    retain: *const c_void,
    release: *const c_void,
    copy_description: *const c_void,
    equal: *const c_void,
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFTypeArrayCallBacks: CFArrayCallBacks;
    static kCFTypeDictionaryKeyCallBacks: CFDictionaryKeyCallBacks;
    static kCFTypeDictionaryValueCallBacks: CFDictionaryValueCallBacks;
    static kCFBooleanTrue: CFTypeRef;
    static kCFBooleanFalse: CFTypeRef;
    fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    fn CFRelease(cf: CFTypeRef);
    fn CFGetTypeID(cf: CFTypeRef) -> CFTypeID;
    fn CFEqual(a: CFTypeRef, b: CFTypeRef) -> u8;
    fn CFStringGetTypeID() -> CFTypeID;
    fn CFArrayGetTypeID() -> CFTypeID;
    fn CFBooleanGetTypeID() -> CFTypeID;
    fn CFAttributedStringGetTypeID() -> CFTypeID;
    fn CFAttributedStringGetString(string: CFTypeRef) -> CFTypeRef;
    fn CFBooleanGetValue(boolean: CFTypeRef) -> u8;
    fn CFStringCreateWithBytes(
        alloc: CFTypeRef,
        bytes: *const u8,
        len: CFIndex,
        encoding: u32,
        external: u8,
    ) -> CFTypeRef;
    fn CFStringGetLength(string: CFTypeRef) -> CFIndex;
    fn CFStringGetMaximumSizeForEncoding(len: CFIndex, encoding: u32) -> CFIndex;
    fn CFStringGetCString(string: CFTypeRef, buffer: *mut c_char, size: CFIndex, encoding: u32) -> u8;
    fn CFArrayCreate(
        alloc: CFTypeRef,
        values: *const CFTypeRef,
        count: CFIndex,
        callbacks: *const CFArrayCallBacks,
    ) -> CFTypeRef;
    fn CFArrayGetCount(array: CFTypeRef) -> CFIndex;
    fn CFArrayGetValueAtIndex(array: CFTypeRef, index: CFIndex) -> CFTypeRef;
    fn CFDictionaryCreate(
        alloc: CFTypeRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        count: CFIndex,
        key_callbacks: *const CFDictionaryKeyCallBacks,
        value_callbacks: *const CFDictionaryValueCallBacks,
    ) -> CFTypeRef;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    static kAXTrustedCheckOptionPrompt: CFTypeRef;
    fn AXUIElementGetTypeID() -> CFTypeID;
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> u8;
    fn AXUIElementCreateApplication(pid: c_int) -> CFTypeRef;
    fn AXUIElementCopyAttributeValue(element: CFTypeRef, attribute: CFTypeRef, value: *mut CFTypeRef) -> AXError;
    fn AXUIElementCopyMultipleAttributeValues(
        element: CFTypeRef,
        attributes: CFTypeRef,
        options: u32,
        values: *mut CFTypeRef,
    ) -> AXError;
    fn AXUIElementSetAttributeValue(element: CFTypeRef, attribute: CFTypeRef, value: CFTypeRef) -> AXError;
    fn AXUIElementSetMessagingTimeout(element: CFTypeRef, seconds: f32) -> AXError;
}

/// A Core Foundation object this code holds a reference to.
struct Cf(CFTypeRef);

impl Cf {
    /// Take over an object returned by a Create or Copy call. Nothing is
    /// made for a null pointer: releasing null stops the whole app.
    fn owned(ptr: CFTypeRef) -> Option<Cf> {
        if ptr.is_null() {
            None
        } else {
            Some(Cf(ptr))
        }
    }
}

impl Drop for Cf {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: this holds one reference, given up exactly once.
            unsafe { CFRelease(self.0) }
        }
    }
}

fn cf_string(text: &str) -> Cf {
    // SAFETY: the bytes are valid UTF-8 for the length given.
    let ptr = unsafe { CFStringCreateWithBytes(null(), text.as_ptr(), text.len() as CFIndex, UTF8, 0) };
    Cf::owned(ptr).expect("a short ASCII string can always be made")
}

/// The text in a string or attributed string, if it is one.
unsafe fn string(value: CFTypeRef) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let kind = CFGetTypeID(value);
    let s = if kind == CFStringGetTypeID() {
        value
    } else if kind == CFAttributedStringGetTypeID() {
        CFAttributedStringGetString(value)
    } else {
        return None;
    };
    let size = CFStringGetMaximumSizeForEncoding(CFStringGetLength(s), UTF8) + 1;
    let mut buffer = vec![0u8; size.max(1) as usize];
    if CFStringGetCString(s, buffer.as_mut_ptr().cast(), size, UTF8) == 0 {
        return None;
    }
    let end = buffer.iter().position(|b| *b == 0).unwrap_or(buffer.len());
    buffer.truncate(end);
    String::from_utf8(buffer).ok()
}

/// The values in an array, if it is one.
unsafe fn items(value: CFTypeRef) -> Vec<CFTypeRef> {
    if value.is_null() || CFGetTypeID(value) != CFArrayGetTypeID() {
        return Vec::new();
    }
    (0..CFArrayGetCount(value)).map(|i| CFArrayGetValueAtIndex(value, i)).collect()
}

/// One element of another app's interface: a window, a tile, some text.
pub struct Element(Cf);

impl Clone for Element {
    fn clone(&self) -> Self {
        // SAFETY: a second reference to a live object.
        Element(Cf(unsafe { CFRetain(self.0 .0) }))
    }
}

/// Two handles to the same element of the same app.
impl PartialEq for Element {
    fn eq(&self, other: &Self) -> bool {
        // SAFETY: both are live elements.
        unsafe { CFEqual(self.0 .0, other.0 .0) != 0 }
    }
}

impl Element {
    /// Hold on to an element read out of another object. Anything that is
    /// not an element, such as an error value in a list, is left alone.
    fn retain(ptr: CFTypeRef) -> Option<Element> {
        // SAFETY: a live object borrowed from its container, checked to be
        // an element before it is kept.
        unsafe {
            if ptr.is_null() || CFGetTypeID(ptr) != AXUIElementGetTypeID() {
                return None;
            }
            Cf::owned(CFRetain(ptr)).map(Element)
        }
    }

    pub fn application(pid: i32) -> Option<Element> {
        // SAFETY: creates a new element for the process.
        Cf::owned(unsafe { AXUIElementCreateApplication(pid) }).map(Element)
    }

    fn copy(&self, attribute: &str) -> Option<Cf> {
        let name = cf_string(attribute);
        let mut value: CFTypeRef = null();
        // SAFETY: valid element and name; the value is returned owned.
        unsafe {
            AXUIElementSetMessagingTimeout(self.0 .0, CALL_TIMEOUT);
            if AXUIElementCopyAttributeValue(self.0 .0, name.0, &mut value) != AX_SUCCESS {
                // Release anything handed back with the error.
                drop(Cf::owned(value));
                return None;
            }
        }
        Cf::owned(value)
    }

    fn bool(&self, attribute: &str) -> Option<bool> {
        let value = self.copy(attribute)?;
        // SAFETY: the type is checked before reading it as a boolean.
        unsafe { (CFGetTypeID(value.0) == CFBooleanGetTypeID()).then(|| CFBooleanGetValue(value.0) != 0) }
    }

    fn set_bool(&self, attribute: &str, on: bool) -> bool {
        let name = cf_string(attribute);
        // SAFETY: valid element, name and a shared boolean constant.
        unsafe {
            AXUIElementSetMessagingTimeout(self.0 .0, CALL_TIMEOUT);
            let value = if on { kCFBooleanTrue } else { kCFBooleanFalse };
            AXUIElementSetAttributeValue(self.0 .0, name.0, value) == AX_SUCCESS
        }
    }

    /// A window's title, or nothing once it has closed.
    pub fn title(&self) -> String {
        // SAFETY: the copied title is a string or nothing.
        self.copy("AXTitle").and_then(|t| unsafe { string(t.0) }).unwrap_or_default()
    }

    /// The app's windows with their titles.
    pub fn windows(&self) -> Vec<(Element, String)> {
        let Some(list) = self.copy("AXWindows") else { return Vec::new() };
        // SAFETY: items are elements borrowed from the array, retained here.
        let windows: Vec<Element> = unsafe { items(list.0) }.into_iter().filter_map(Element::retain).collect();
        windows
            .into_iter()
            .map(|w| {
                // SAFETY: the copied title is a string or nothing.
                let title = w.copy("AXTitle").and_then(|t| unsafe { string(t.0) }).unwrap_or_default();
                (w, title)
            })
            .collect()
    }
}

/// Whether macOS lets Tidy read other apps' windows.
pub fn trusted() -> bool {
    // SAFETY: no arguments.
    unsafe { AXIsProcessTrusted() != 0 }
}

/// Ask macOS for the permission, which shows its own prompt the first time.
pub fn ask() -> bool {
    // SAFETY: a one-entry dictionary of shared constants.
    unsafe {
        let keys = [kAXTrustedCheckOptionPrompt];
        let values = [kCFBooleanTrue];
        let options = Cf::owned(CFDictionaryCreate(
            null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        ));
        AXIsProcessTrustedWithOptions(options.as_ref().map_or(null(), |o| o.0)) != 0
    }
}

/// The switches that ask an app to share its web page's elements: the one
/// VoiceOver sets, which Chrome and Edge honour, and the one Electron apps
/// and Teams honour.
const SWITCHES: [&str; 2] = ["AXEnhancedUserInterface", "AXManualAccessibility"];

/// How an app's switches were before Tidy touched them.
pub struct Switches {
    before: Vec<(&'static str, Option<bool>)>,
}

pub fn switches(app: &Element) -> Switches {
    Switches { before: SWITCHES.iter().map(|name| (*name, app.bool(name))).collect() }
}

/// Ask a Chromium-based app to share its web page's elements, as it does for
/// VoiceOver. Chrome can stop sharing while its switch still reads as on,
/// and setting it on again brings the page back. When that has not worked,
/// `firmly` turns each switch off first, so the app sees it change. Chrome
/// answers with an error even when it listens, so the answer is not
/// trusted: whether the page appears on the next read is what counts. The
/// page takes a second or two.
pub fn wake_web_tree(app: &Element, firmly: bool) {
    for name in SWITCHES {
        if firmly {
            app.set_bool(name, false);
        }
        app.set_bool(name, true);
    }
}

/// Put an app's switches back as they were: off, unless something else,
/// such as VoiceOver, had turned them on.
pub fn put_back(app: &Element, switches: &Switches) {
    for (name, before) in &switches.before {
        if *before != Some(true) {
            app.set_bool(name, false);
        }
    }
}

const FIELDS: [&str; 7] =
    ["AXRole", "AXSubrole", "AXTitle", "AXDescription", "AXValue", "AXDOMClassList", "AXChildren"];

/// Reads elements into plain trees, within a time and size budget.
pub struct Reader {
    fields: Cf,
    deadline: Instant,
    left: usize,
    /// The other app stopped answering.
    pub stopped: bool,
    /// Every element read, by the handle its node carries.
    pub elements: Vec<Element>,
}

impl Reader {
    pub fn new(budget: Duration, max_nodes: usize) -> Reader {
        let names: Vec<Cf> = FIELDS.iter().map(|f| cf_string(f)).collect();
        let raw: Vec<CFTypeRef> = names.iter().map(|n| n.0).collect();
        // SAFETY: the array retains each name it is given.
        let fields = Cf::owned(unsafe {
            CFArrayCreate(null(), raw.as_ptr(), raw.len() as CFIndex, &kCFTypeArrayCallBacks)
        })
        .expect("a small array can always be made");
        Reader { fields, deadline: Instant::now() + budget, left: max_nodes, stopped: false, elements: Vec::new() }
    }

    pub fn tree(&mut self, element: &Element) -> Node {
        self.node(element, 0)
    }

    fn node(&mut self, element: &Element, depth: usize) -> Node {
        let mut node = Node::default();
        if self.stopped || self.left == 0 || Instant::now() >= self.deadline {
            return node;
        }
        self.left -= 1;
        node.handle = Some(self.elements.len());
        self.elements.push(element.clone());

        let mut out: CFTypeRef = null();
        // SAFETY: valid element and attribute list; the result is owned.
        let error = unsafe {
            AXUIElementSetMessagingTimeout(element.0 .0, CALL_TIMEOUT);
            AXUIElementCopyMultipleAttributeValues(element.0 .0, self.fields.0, 0, &mut out)
        };
        let values = Cf::owned(out);
        if error == AX_CANNOT_COMPLETE {
            self.stopped = true;
            return node;
        }
        let Some(values) = values.filter(|_| error == AX_SUCCESS) else { return node };
        // SAFETY: each value is read for its type before use. Attributes an
        // element lacks come back as error values, which read as nothing.
        let children: Vec<Element> = unsafe {
            let all = items(values.0);
            if all.len() != FIELDS.len() {
                return node;
            }
            node.role = string(all[0]).unwrap_or_default();
            node.subrole = string(all[1]).unwrap_or_default();
            node.title = string(all[2]).unwrap_or_default();
            node.desc = string(all[3]).unwrap_or_default();
            node.value = string(all[4]).unwrap_or_default();
            node.classes = items(all[5]).into_iter().filter_map(|c| string(c)).collect::<Vec<_>>().join(" ");
            items(all[6]).into_iter().filter_map(Element::retain).collect()
        };
        if depth < MAX_DEPTH {
            for child in &children {
                node.children.push(self.node(child, depth + 1));
            }
        }
        node
    }
}

/// Running apps, as macOS lists them: process id and bundle identifier,
/// such as "com.google.Chrome". The identifier holds however the app was
/// started, even when it runs from a temporary copy while it updates, as
/// Chrome does.
pub fn running_apps() -> Vec<(i32, String)> {
    use objc2_app_kit::NSWorkspace;
    // The watch runs on its own thread, which has no pool of its own for
    // the objects macOS hands back.
    objc2::rc::autoreleasepool(|_| {
        NSWorkspace::sharedWorkspace()
            .runningApplications()
            .iter()
            .filter_map(|app| Some((app.processIdentifier(), app.bundleIdentifier()?.to_string())))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_value_is_nothing_and_is_never_released() {
        // Releasing a null reference stops the process, so this test would
        // take the whole test run down with it.
        assert!(Cf::owned(null()).is_none());
        assert!(Element::retain(null()).is_none());
        // SAFETY: null is handled before anything is read.
        unsafe {
            assert!(string(null()).is_none());
            assert!(items(null()).is_empty());
        }
        drop(Cf(null()));
    }

    #[test]
    fn an_app_that_gives_no_answer_reads_as_empty() {
        // No process has this id, so every call fails and hands back nothing.
        let app = Element::application(i32::MAX - 7).expect("an element is made for any id");
        assert!(app.windows().is_empty());
        assert_eq!(app.bool("AXManualAccessibility"), None);
        assert!(!app.set_bool("AXManualAccessibility", true));
        let before = switches(&app);
        assert!(before.before.iter().all(|(_, value)| value.is_none()));
        wake_web_tree(&app, false);
        wake_web_tree(&app, true);
        put_back(&app, &before);
        assert!(app.title().is_empty());
        assert!(app == app.clone());
        assert!(app != Element::application(i32::MAX - 8).unwrap());
        let mut reader = Reader::new(Duration::from_millis(500), 100);
        let tree = reader.tree(&app);
        assert!(tree.role.is_empty() && tree.children.is_empty());
    }

    #[test]
    fn only_elements_are_kept_as_elements() {
        let text = cf_string("not an element");
        assert!(Element::retain(text.0).is_none());
        assert!(Element::retain(null()).is_none());
        let app = Element::application(i32::MAX - 7).expect("an element");
        assert!(Element::retain(app.0 .0).is_some());
    }

    #[test]
    fn strings_come_back_whole() {
        let made = cf_string("Zoë Müller, video is on");
        // SAFETY: a string made just above.
        assert_eq!(unsafe { string(made.0) }.as_deref(), Some("Zoë Müller, video is on"));
    }
}
