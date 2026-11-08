//! The SDR white level of the monitor the window is on (canon row 184, found on rhombus's HDR monitor 2026-11-08: "it's for
//! sure dimmer"). On Windows the scRGB surface's 1.0 is 80 nits, while the desktop shows SDR content at the user's "SDR content
//! brightness"; our 1.0 is SDR white (as on macOS EDR), so the screen pass scales by that level ÷ 80. Read through
//! DisplayConfig: the source whose GDI name is the window's monitor → its target's DISPLAYCONFIG_SDR_WHITE_LEVEL
//! (SDRWhiteLevel / 1000 × 80 nits). macOS: EDR already puts SDR white at 1.0 — gain 1.

/// The output gain for the monitor named `gdi_name` ("\\.\DISPLAY1"), or 1.0 when it cannot be read.
#[cfg(windows)]
pub fn gain_for(gdi_name: &str) -> f32 {
    use windows::Win32::Devices::Display::*;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    unsafe {
        let (mut np, mut nm) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm) != ERROR_SUCCESS { return 1.0 }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        if QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut np, paths.as_mut_ptr(), &mut nm, modes.as_mut_ptr(), None) != ERROR_SUCCESS { return 1.0 }
        paths.truncate(np as usize);
        for p in &paths {
            let mut src = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
            src.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
            src.header.size = std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
            src.header.adapterId = p.sourceInfo.adapterId;
            src.header.id = p.sourceInfo.id;
            if DisplayConfigGetDeviceInfo(&mut src.header) != 0 { continue }
            let n = src.viewGdiDeviceName.iter().position(|c| *c == 0).unwrap_or(src.viewGdiDeviceName.len());
            if String::from_utf16_lossy(&src.viewGdiDeviceName[..n]) != gdi_name { continue }
            let mut w = DISPLAYCONFIG_SDR_WHITE_LEVEL::default();
            w.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL;
            w.header.size = std::mem::size_of::<DISPLAYCONFIG_SDR_WHITE_LEVEL>() as u32;
            w.header.adapterId = p.targetInfo.adapterId;
            w.header.id = p.targetInfo.id;
            if DisplayConfigGetDeviceInfo(&mut w.header) != 0 { return 1.0 }
            // SDRWhiteLevel is in 1/1000 of 80 nits: 1000 = 80 nits (the scRGB reference) → gain 1
            let g = w.SDRWhiteLevel as f32 / 1000.0;
            return if g.is_finite() && g > 0.1 { g } else { 1.0 }
        }
        1.0
    }
}
#[cfg(not(windows))]
pub fn gain_for(_gdi_name: &str) -> f32 { 1.0 }
