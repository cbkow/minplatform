//! S7 — the Windows stream's surface (docs/spikes/S7-winstream.md). Stage A: which hardware H.264 encoders are here, what
//! they say about D3D11 / D3D12, and whether a Media Foundation device manager takes wgpu's own D3D12 device.
#[cfg(not(windows))]
fn main() { eprintln!("s7-winstream: Windows only"); }

#[cfg(windows)]
fn main() -> windows::core::Result<()> {
    use windows::core::{Interface, GUID, PWSTR};
    use windows::Win32::Media::MediaFoundation::*;
    use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_MULTITHREADED};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
        // wgpu's own DX12 device
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends: wgpu::Backends::DX12, ..wgpu::InstanceDescriptor::new_without_display_handle() });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default() })).expect("a DX12 adapter");
        println!("adapter: {:?}", adapter.get_info().name);
        let (device, _queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("a device");
        let d3d12 = { let hal = device.as_hal::<wgpu::hal::api::Dx12>().expect("the DX12 hal"); hal.raw_device().clone() };

        let out = MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Video, guidSubtype: MFVideoFormat_H264 };
        let (mut list, mut n) = (std::ptr::null_mut::<Option<IMFActivate>>(), 0u32);
        MFTEnumEx(MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER, None, Some(&out), &mut list, &mut n)?;
        println!("hardware H.264 encoder MFTs: {n}");
        let acts: Vec<IMFActivate> = (0..n as usize).filter_map(|i| (*list.add(i)).take()).collect();
        CoTaskMemFree(Some(list as *const _));
        let u32_of = |a: &IMFAttributes, k: &GUID| a.GetUINT32(k).map(|v| v.to_string()).unwrap_or_else(|_| "—".into());
        for (i, act) in acts.iter().enumerate() {
            let (mut name, mut len) = (PWSTR::null(), 0u32);
            let _ = act.GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &mut name, &mut len);
            let friendly = if name.is_null() { "?".to_string() } else { name.to_string().unwrap_or_default() };
            println!("[{i}] {friendly}");
            let mft: IMFTransform = match act.ActivateObject() { Ok(m) => m, Err(e) => { println!("    activate failed: {e}"); continue } };
            let Ok(attrs) = mft.GetAttributes() else { println!("    no attributes"); continue };
            println!("    async {}  D3D-aware {}  D3D11-aware {}", u32_of(&attrs, &MF_TRANSFORM_ASYNC), u32_of(&attrs, &MF_SA_D3D_AWARE), u32_of(&attrs, &MF_SA_D3D11_AWARE));
            let _ = attrs.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1);
            // a device manager on wgpu's D3D12 device → the MFT
            let (mut token, mut mgr) = (0u32, None::<IMFDXGIDeviceManager>);
            MFCreateDXGIDeviceManager(&mut token, &mut mgr)?;
            let mgr = mgr.unwrap();
            match mgr.ResetDevice(&d3d12, token) { Ok(()) => println!("    device manager took the D3D12 device"), Err(e) => { println!("    device manager REFUSED the D3D12 device: {e}"); continue } }
            match mft.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, mgr.as_raw() as usize) { Ok(()) => println!("    the MFT ACCEPTED the D3D12 device manager"), Err(e) => println!("    the MFT refused the D3D12 device manager: {e}") }
        }
        MFShutdown()?;
    }
    Ok(())
}
