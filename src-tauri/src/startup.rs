#[tauri::command]
pub(crate) fn desktop_autostart(enabled: bool) -> Result<(), String> {
    if osdm_browser_bridge::native_test_directory()
        .map_err(|_| "Invalid native test directory")?
        .is_some()
    {
        return Err("Startup registration is disabled during native browser tests".into());
    }
    #[cfg(windows)]
    {
        let current = std::env::current_exe().map_err(|_| "Cannot locate OSDM executable")?;
        let key = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
            .create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
            .map_err(|_| "Cannot open Windows startup settings")?
            .0;
        if enabled {
            key.set_value("OSDM", &format!("\"{}\"", current.display()))
                .map_err(|_| "Cannot enable startup")?;
        } else if let Err(error) = key.delete_value("OSDM")
            && error.kind() != std::io::ErrorKind::NotFound
        {
            return Err("Cannot disable startup".into());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
        Err("Start with Windows requires Windows".into())
    }
}

#[tauri::command]
pub(crate) fn desktop_autostart_status() -> bool {
    if osdm_browser_bridge::native_test_directory()
        .ok()
        .flatten()
        .is_some()
    {
        return false;
    }
    #[cfg(windows)]
    {
        winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
            .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
            .and_then(|key| key.get_value::<String, _>("OSDM"))
            .is_ok()
    }
    #[cfg(not(windows))]
    {
        false
    }
}
