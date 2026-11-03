//! Native macOS word correction for the custom composer text surface.
#[cfg(target_os = "macos")]
pub(super) fn correction(word: &str) -> Option<String> {
    use objc2::{class, msg_send, rc::Retained, runtime::AnyObject};
    use objc2_foundation::{NSRange, NSString};
    // NSSpellChecker's singleton is used only on GPUI's UI thread.
    unsafe {
        let checker: Retained<AnyObject> = msg_send![class!(NSSpellChecker), sharedSpellChecker];
        let language: Retained<NSString> = msg_send![&checker, language];
        let input = NSString::from_str(word);
        let corrected: Option<Retained<NSString>> = msg_send![&checker,correctionForWordRange:NSRange::new(0,word.encode_utf16().count()), inString:&*input, language:&*language, inSpellDocumentWithTag:0isize];
        corrected.map(|s| s.to_string()).filter(|s| s != word)
    }
}
#[cfg(not(target_os = "macos"))]
pub(super) fn correction(_word: &str) -> Option<String> {
    None
}
