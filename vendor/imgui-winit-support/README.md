# imgui-winit-support (vendored patch)

Vendored copy of imgui-winit-support 0.13.0 with an Android compatibility fix.

Upstream gates KeyEventExtModifierSupplement::key_without_modifiers to
windows/macos/x11/wayland/orbital, so it is absent on Android and 0.13.0 fails
to build for aarch64-linux-android. This copy imports and calls it only on
non-Android targets and uses event.logical_key on Android.

Licensed MIT OR Apache-2.0, same as upstream.
