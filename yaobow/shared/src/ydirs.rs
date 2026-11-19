use std::path::PathBuf;

pub fn save_dir() -> PathBuf {
    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
    {
        dirs::data_dir().unwrap().join("yaobow")
    }

    #[cfg(target_os = "android")]
    {
        android_base_dir().join("yaobow")
    }

    #[cfg(vita)]
    {
        PathBuf::from("ux0:yaobow")
    }
}

pub fn config_dir() -> PathBuf {
    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
    {
        dirs::config_dir().unwrap().join("yaobow")
    }

    // `dirs::config_dir()` is `None` on Android, so keep config next to
    // the save data in the app's own storage.
    #[cfg(target_os = "android")]
    {
        android_base_dir().join("yaobow")
    }

    #[cfg(vita)]
    {
        PathBuf::from("ux0:yaobow")
    }
}

/// App-private storage on Android: the external files dir when present
/// (`/sdcard/Android/data/<package>/files`), else the internal one.
#[cfg(target_os = "android")]
pub fn android_base_dir() -> PathBuf {
    let app = radiance::android::android_app();
    app.external_data_path()
        .or_else(|| app.internal_data_path())
        .unwrap_or_else(|| PathBuf::from("/data/local/tmp"))
}
