//! Android glue.
//!
//! winit 0.30 on Android needs the `AndroidApp` that `android-activity`
//! hands to the app's `android_main` entry point. The game crate stores
//! it here once at startup; the winit platform layer and the save/config
//! directory helpers read it back.
//!
//! The event loop and `android_main` run on the same thread, so a
//! thread-local is enough and avoids requiring `Send + Sync`.

use std::cell::RefCell;

pub use winit::platform::android::activity::AndroidApp;

thread_local! {
    static ANDROID_APP: RefCell<Option<AndroidApp>> = const { RefCell::new(None) };
}

/// Store the `AndroidApp` received by `android_main`. Must be called
/// before the engine creates its event loop.
pub fn set_android_app(app: AndroidApp) {
    ANDROID_APP.with(|slot| *slot.borrow_mut() = Some(app));
}

/// The `AndroidApp` stored by [`set_android_app`].
///
/// Panics if `android_main` did not store it first.
pub fn android_app() -> AndroidApp {
    ANDROID_APP.with(|slot| {
        slot.borrow()
            .clone()
            .expect("android_main must call radiance::android::set_android_app first")
    })
}
