//! The macOS Accessibility calls used to read a call window: the same
//! information VoiceOver reads. Tidy only reads, apart from one switch that
//! Chromium-based apps (Chrome, Edge and Teams itself) need before they share
//! a web page's elements, which is put back when the recording ends.
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

extern "C" {
    fn proc_listallpids(buffer: *mut c_void, size: c_int) -> c_int;
    fn proc_pidpath(pid: c_int, buffer: *mut c_void, size: u32) -> c_int;
}

/// A Core Foundation object this code holds a reference to.
struct Cf(CFTypeRef);

impl Cf {
    /// Take over an object returned by a Create or Copy call.
    fn owned(ptr: CFTypeRef) -> Option<Cf> {
        (!ptr.is_null()).then_some(Cf(ptr))
    }
}

impl Drop for Cf {
    fn drop(&mut self) {
        // SAFETY: this holds one reference, given up exactly once.
        unsafe { CFRelease(self.0) }
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

impl Element {
    /// Hold on to an element read out of another object.
    fn retain(ptr: CFTypeRef) -> Option<Element> {
        // SAFETY: a live object borrowed from its container.
        (!ptr.is_null()).then(|| Element(Cf(unsafe { CFRetain(ptr) })))
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
                return Cf::owned(value).and(None);
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

/// Ask a Chromium-based app to share its web page's elements, as it does for
/// VoiceOver. Returns whether anything changed, and the switches to put back
/// afterwards: only those that were readable as off beforehand.
pub fn share_web_tree(app: &Element) -> (bool, Vec<&'static str>) {
    let mut changed = false;
    let mut put_back = Vec::new();
    for name in ["AXManualAccessibility", "AXEnhancedUserInterface"] {
        let before = app.bool(name);
        if before == Some(true) {
            continue;
        }
        if app.set_bool(name, true) {
            changed = true;
            if before == Some(false) {
                put_back.push(name);
            }
        }
    }
    (changed, put_back)
}

pub fn put_back(app: &Element, switches: &[&'static str]) {
    for name in switches {
        app.set_bool(name, false);
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

/// Running apps' main processes: process id and the app's name, such as
/// "Google Chrome" for /Applications/Google Chrome.app/Contents/MacOS/….
pub fn running_apps() -> Vec<(i32, String)> {
    // SAFETY: a null buffer asks only for the count.
    let count = unsafe { proc_listallpids(std::ptr::null_mut(), 0) };
    if count <= 0 {
        return Vec::new();
    }
    let mut pids = vec![0 as c_int; count as usize + 32];
    // SAFETY: the buffer holds as many ids as its size in bytes says.
    let found = unsafe { proc_listallpids(pids.as_mut_ptr().cast(), (pids.len() * 4) as c_int) };
    pids.truncate(found.max(0) as usize);
    let mut apps = Vec::new();
    for pid in pids {
        let mut buffer = vec![0u8; 4096];
        // SAFETY: the buffer is as large as the size passed.
        let written = unsafe { proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
        if written <= 0 {
            continue;
        }
        let path = String::from_utf8_lossy(&buffer[..written as usize]).into_owned();
        if let Some(name) = main_app(&path) {
            apps.push((pid, name));
        }
    }
    apps
}

/// The app a process is the main program of, from its path: helpers inside
/// the bundle (such as Chrome's tab processes) are not.
fn main_app(path: &str) -> Option<String> {
    let at = path.find(".app/Contents/MacOS/")?;
    let executable = &path[at + ".app/Contents/MacOS/".len()..];
    if executable.contains('/') {
        return None;
    }
    let bundle = &path[..at];
    // A helper app inside another app's bundle is not a main program.
    if bundle.contains(".app/") {
        return None;
    }
    Some(bundle.rsplit('/').next()?.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_apps_main_program_counts() {
        assert_eq!(main_app("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome").as_deref(), Some("Google Chrome"));
        assert_eq!(
            main_app("/Applications/Microsoft Teams.app/Contents/MacOS/MSTeams").as_deref(),
            Some("Microsoft Teams")
        );
        assert_eq!(
            main_app("/Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Versions/1/Helpers/Google Chrome Helper (Renderer).app/Contents/MacOS/Google Chrome Helper (Renderer)"),
            None
        );
        assert_eq!(main_app("/usr/sbin/coreaudiod"), None);
    }
}
