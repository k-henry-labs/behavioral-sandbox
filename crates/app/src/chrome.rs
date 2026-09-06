//! The window's own chrome, where the toolkit stops and the platform begins.
//!
//! - **macOS lays a titlebar out from what the window carries.** A window with a toolbar gets the
//!   taller one, and its close, minimise and zoom buttons sit on that line; a window without gets
//!   the short one, wherever its content is drawn. Nothing in iced's window settings moves them:
//!   `title_hidden`, `titlebar_transparent` and `fullsize_content_view` were each measured on this
//!   host and left the three buttons at 9 from the top edge.
//! - **So the window is given an empty toolbar**, which is what a native window of this shape
//!   carries, and macOS itself puts the buttons on the line a head is drawn to.
//! - **AppKit is Objective-C**, which is why this module is the app's one `unsafe`, as libkrun's C
//!   is `bsx-krun`'s. It is one call: everything read back out is the platform's own.

/// Puts the window's own buttons on the line a head is drawn to, on the platform that moves them.
pub(crate) fn unify_titlebar<T: Send + 'static>(id: iced::window::Id) -> iced::Task<T> {
    iced::window::run(id, |window| {
        #[cfg(target_os = "macos")]
        macos::give_the_window_a_toolbar(window);
        #[cfg(not(target_os = "macos"))]
        let _ = window;
    })
    .discard()
}

#[cfg(target_os = "macos")]
mod macos {
    use iced::window::raw_window_handle::RawWindowHandle;
    use objc2::rc::Retained;
    use objc2_app_kit::{NSToolbar, NSView, NSWindow, NSWindowToolbarStyle};
    use objc2_foundation::MainThreadMarker;

    /// Gives the window an empty toolbar in the unified style, the shape that makes AppKit lay the
    /// titlebar out at a head's height. A window that cannot be reached is left as it is.
    pub(super) fn give_the_window_a_toolbar(window: &dyn iced::window::Window) {
        let Some(window) = appkit_window(window) else {
            return;
        };
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        // SAFETY: every call here is an AppKit setter on a window this thread owns, holding the
        // main-thread marker those classes require. `NSToolbar::new` is the empty toolbar.
        #[allow(unsafe_code)]
        unsafe {
            window.setToolbar(Some(&NSToolbar::new(mtm)));
            window.setToolbarStyle(NSWindowToolbarStyle::Unified);
        }
    }

    /// The `NSWindow` behind an iced window, or `None` where the handle is not AppKit's.
    fn appkit_window(window: &dyn iced::window::Window) -> Option<Retained<NSWindow>> {
        let handle = window.window_handle().ok()?;
        let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
            return None;
        };
        // SAFETY: the handle is that window's live `NSView` for as long as this call runs, and
        // `Retained::retain` takes its own reference over the borrow.
        #[allow(unsafe_code)]
        let view: Retained<NSView> = unsafe { Retained::retain(appkit.ns_view.as_ptr().cast()) }?;
        view.window()
    }
}
