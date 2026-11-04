//! Which call apps are using the microphone right now. macOS keeps a list of
//! the processes holding audio input, readable without any extra permission,
//! so Tidy can offer to record when a call starts and notice when it ends.

/// Apps that hold the microphone for calls, matched against the `.app` bundle
/// (or helper) a process runs from. Browsers are included because Google Meet
/// and many other calls run in one.
const CALL_APPS: &[(&str, &str)] = &[
    ("zoom.us", "Zoom"),
    ("microsoft teams", "Teams"),
    ("facetime", "FaceTime"),
    ("slack", "Slack"),
    ("discord", "Discord"),
    ("webex", "Webex"),
    ("whatsapp", "WhatsApp"),
    ("signal", "Signal"),
    ("telegram", "Telegram"),
    ("skype", "Skype"),
    ("gotomeeting", "GoTo Meeting"),
    ("goto", "GoTo"),
    ("ringcentral", "RingCentral"),
    ("around", "Around"),
    ("tuple", "Tuple"),
    ("google chrome", "Chrome"),
    ("microsoft edge", "Edge"),
    ("firefox", "Firefox"),
    ("brave browser", "Brave"),
    ("arc", "Arc"),
    ("vivaldi", "Vivaldi"),
    ("opera", "Opera"),
    ("safari", "Safari"),
    // Safari's audio runs in WebKit's media process, not in Safari.app.
    ("com.apple.webkit.gpu", "Safari"),
];

/// The call app a process belongs to, from its path on disk: the outermost
/// `.app` bundle, or the WebKit media process Safari uses.
pub fn app_name(path: &str) -> Option<&'static str> {
    let lower = path.to_lowercase();
    let bundle = lower
        .split('/')
        .find(|part| part.ends_with(".app") || part.ends_with(".xpc"))?;
    let bundle = bundle.trim_end_matches(".app").trim_end_matches(".xpc");
    CALL_APPS
        .iter()
        .find(|(key, _)| bundle == *key || bundle.starts_with(&format!("{key} ")))
        .map(|(_, name)| *name)
}

/// Names of the call apps holding the microphone, without duplicates.
#[cfg(target_os = "macos")]
pub fn mic_apps() -> Vec<&'static str> {
    let own = std::process::id() as i32;
    let mut names: Vec<&'static str> = Vec::new();
    for (pid, running) in macos::input_processes() {
        if !running || pid == own || pid <= 0 {
            continue;
        }
        if let Some(name) = macos::process_path(pid).as_deref().and_then(app_name) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

#[cfg(not(target_os = "macos"))]
pub fn mic_apps() -> Vec<&'static str> {
    Vec::new()
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;
    use std::ptr::NonNull;

    use objc2_core_audio::{
        kAudioHardwarePropertyProcessObjectList, kAudioObjectPropertyElementMain,
        kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, kAudioProcessPropertyIsRunningInput,
        kAudioProcessPropertyPID, AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize,
        AudioObjectID, AudioObjectPropertyAddress, AudioObjectPropertySelector,
    };

    fn address(selector: AudioObjectPropertySelector) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress {
            mSelector: selector,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        }
    }

    fn read<T: Copy + Default>(object: AudioObjectID, selector: AudioObjectPropertySelector) -> Option<T> {
        let mut address = address(selector);
        let mut value = T::default();
        let mut size = std::mem::size_of::<T>() as u32;
        // SAFETY: the address, size and value pointers are valid for the call
        // and `size` matches the buffer behind `value`.
        let status = unsafe {
            AudioObjectGetPropertyData(
                object,
                NonNull::from(&mut address),
                0,
                std::ptr::null(),
                NonNull::from(&mut size),
                NonNull::from(&mut value).cast::<c_void>(),
            )
        };
        (status == 0).then_some(value)
    }

    /// Every audio process the system knows of: its PID, and whether it is
    /// capturing input at the moment.
    pub fn input_processes() -> Vec<(i32, bool)> {
        let system = kAudioObjectSystemObject as AudioObjectID;
        let mut list = address(kAudioHardwarePropertyProcessObjectList);
        let mut size = 0u32;
        // SAFETY: valid pointers for the duration of the call.
        let status = unsafe {
            AudioObjectGetPropertyDataSize(system, NonNull::from(&mut list), 0, std::ptr::null(), NonNull::from(&mut size))
        };
        if status != 0 || size == 0 {
            return Vec::new();
        }
        let count = size as usize / std::mem::size_of::<AudioObjectID>();
        let mut objects = vec![0 as AudioObjectID; count];
        // SAFETY: `objects` holds exactly `size` bytes.
        let status = unsafe {
            AudioObjectGetPropertyData(
                system,
                NonNull::from(&mut list),
                0,
                std::ptr::null(),
                NonNull::from(&mut size),
                NonNull::new(objects.as_mut_ptr().cast::<c_void>()).expect("vector pointer"),
            )
        };
        if status != 0 {
            return Vec::new();
        }
        objects.truncate(size as usize / std::mem::size_of::<AudioObjectID>());
        objects
            .into_iter()
            .filter_map(|object| {
                let pid = read::<i32>(object, kAudioProcessPropertyPID)?;
                let running = read::<u32>(object, kAudioProcessPropertyIsRunningInput).unwrap_or(0) != 0;
                Some((pid, running))
            })
            .collect()
    }

    /// The executable a process runs, such as
    /// "/Applications/zoom.us.app/Contents/MacOS/zoom.us".
    pub fn process_path(pid: i32) -> Option<String> {
        let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        // SAFETY: the buffer is writable and its length is passed alongside.
        let written = unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
        if written <= 0 {
            return None;
        }
        buffer.truncate(written as usize);
        String::from_utf8(buffer).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::app_name;

    #[test]
    fn call_apps_are_named_from_their_bundle() {
        assert_eq!(app_name("/Applications/zoom.us.app/Contents/MacOS/zoom.us"), Some("Zoom"));
        assert_eq!(
            app_name("/Applications/Microsoft Teams.app/Contents/Helpers/Microsoft Teams WebView.app/Contents/MacOS/x"),
            Some("Teams")
        );
        assert_eq!(
            app_name("/Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Helpers/Google Chrome Helper.app/Contents/MacOS/Google Chrome Helper"),
            Some("Chrome")
        );
        assert_eq!(
            app_name("/System/Library/Frameworks/WebKit.framework/Versions/A/XPCServices/com.apple.WebKit.GPU.xpc/Contents/MacOS/com.apple.WebKit.GPU"),
            Some("Safari")
        );
        assert_eq!(app_name("/System/Applications/FaceTime.app/Contents/MacOS/FaceTime"), Some("FaceTime"));
    }

    #[test]
    #[ignore = "reads this Mac's audio processes"]
    fn lists_this_macs_audio_processes() {
        let processes = super::macos::input_processes();
        for (pid, running) in &processes {
            let path = super::macos::process_path(*pid).unwrap_or_default();
            eprintln!("{pid} input={running} {path}");
        }
        eprintln!("call apps on the mic: {:?}", super::mic_apps());
        assert!(!processes.is_empty());
    }

    #[test]
    fn other_apps_using_the_mic_are_not_calls() {
        assert_eq!(app_name("/Applications/Voice Memos.app/Contents/MacOS/VoiceMemos"), None);
        assert_eq!(app_name("/Applications/Tidy.app/Contents/MacOS/tidy"), None);
        assert_eq!(app_name("/usr/libexec/corespeechd"), None);
        // "Arc" must not match apps that merely start with those letters.
        assert_eq!(app_name("/Applications/Archive Utility.app/Contents/MacOS/x"), None);
    }
}
