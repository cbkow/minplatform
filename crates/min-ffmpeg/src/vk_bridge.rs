//! Windows (S6, canon row 149): the Vulkan → D3D12 bridge. FFmpeg decodes ProRes / FFV1 / APV on ITS Vulkan device
//! (`HwDevice::vulkan`); this hands every decoded `AVVkFrame` to the DX12 presenter with no round trip through system memory:
//!
//! 1. the presenter's `ID3D12Device` creates one SHARED committed texture per plane (R16 / R16G16, or R8 / R8G8 for 8-bit
//!    sources) in a small ring, and one shared `ID3D12Fence`;
//! 2. Vulkan imports each texture as external memory (`VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE_BIT`, a dedicated
//!    allocation) and the fence as a timeline semaphore (`VK_EXTERNAL_SEMAPHORE_HANDLE_TYPE_D3D12_FENCE_BIT`);
//! 3. per frame, one command buffer on one of FFmpeg's queues: barriers, `vkCmdCopyImage` per plane (a multiplane image's
//!    `PLANE_n` aspects, or FFmpeg's separate per-plane images, straight into the single-plane D3D12 textures — the 10X6 /
//!    12X4 packed formats copy into R16 bit-for-bit, so the presenter reads them as 16-bit MSB-aligned like P010), the
//!    source back to the layout FFmpeg expects;
//! 4. the submit waits on the frame's own timeline semaphores at `sem_value` and signals them at `sem_value + 1` (FFmpeg's
//!    contract), and signals the shared semaphore at the ring's next value; the presenter waits on the D3D12 fence at that
//!    value before sampling (`Dx12Import::import` → `add_wait_fence`).
//!
//! QCView's `D3D11VulkanDecodeBridge` did the same across D3D11 with a compute pass; the copy is `vkCmdCopyImage` here because
//! every plane pair is size-compatible. The ring holds the `AVFrame` references until their slot is reused after the copy has
//! completed, so FFmpeg's pool slot is never returned early.
use crate::{ffmpeg::HwFrame, hw::HwDevice};
use ash::vk::{self, Handle};
use ffmpeg_sys_next as ff;
use min_present::Dx12FrameHandle;
use std::ffi::c_void;
use windows::core::Interface;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

const RING: usize = 4;

struct PlaneRes { res: ID3D12Resource, raw: *mut c_void, image: vk::Image, memory: vk::DeviceMemory, w: u32, h: u32, _handle: HANDLE }
struct Slot { planes: Vec<PlaneRes>, key: (i32, u32, u32, u8), busy_until: u64, keep: Option<HwFrame>, cmd: vk::CommandBuffer }

pub struct VkBridge {
    instance: ash::Instance, device: ash::Device, ext_sem: ash::khr::external_semaphore_win32::Device,
    hwdev: *mut ff::AVHWDeviceContext, vkctx: *const ff::AVVulkanDeviceContext,
    qf: u32, queue: vk::Queue, pool: vk::CommandPool, mem_props: vk::PhysicalDeviceMemoryProperties,
    d3d: ID3D12Device, fence: ID3D12Fence, fence_raw: *mut c_void, sem: vk::Semaphore, fence_value: u64,
    slots: Vec<Slot>, next: usize, logged: bool,
    _hw: HwDevice,
}
unsafe impl Send for VkBridge {}

/// What one decoded frame looks like on the Vulkan side, and the plane set the presenter gets.
struct Layout { n_img: usize, multiplane_2: bool, planes: Vec<(u32, u32, bool)>, depth: u8, sw_format: ff::AVPixelFormat, w: u32, h: u32 }

fn two_plane(f: vk::Format) -> bool {
    matches!(f, vk::Format::G8_B8R8_2PLANE_420_UNORM | vk::Format::G8_B8R8_2PLANE_422_UNORM | vk::Format::G8_B8R8_2PLANE_444_UNORM
        | vk::Format::G10X6_B10X6R10X6_2PLANE_420_UNORM_3PACK16 | vk::Format::G10X6_B10X6R10X6_2PLANE_422_UNORM_3PACK16 | vk::Format::G10X6_B10X6R10X6_2PLANE_444_UNORM_3PACK16
        | vk::Format::G12X4_B12X4R12X4_2PLANE_420_UNORM_3PACK16 | vk::Format::G12X4_B12X4R12X4_2PLANE_422_UNORM_3PACK16 | vk::Format::G12X4_B12X4R12X4_2PLANE_444_UNORM_3PACK16
        | vk::Format::G16_B16R16_2PLANE_420_UNORM | vk::Format::G16_B16R16_2PLANE_422_UNORM | vk::Format::G16_B16R16_2PLANE_444_UNORM)
}

impl VkBridge {
    /// Build on FFmpeg's Vulkan device (`hw`, kind Vulkan) and the presenter's `ID3D12Device` the device carries.
    pub fn new(hw: &HwDevice) -> Option<VkBridge> {
        let d3d_raw = hw.d3d_device();
        if d3d_raw.is_null() { log::warn!("vk bridge: no D3D12 device on the Vulkan HwDevice — frames will be read back"); return None }
        unsafe {
            let hwdev = (*hw.raw()).data as *mut ff::AVHWDeviceContext;
            let vkctx = (*hwdev).hwctx as *const ff::AVVulkanDeviceContext;
            let gipa = (*vkctx).get_proc_addr?;
            let static_fn = ash::StaticFn { get_instance_proc_addr: std::mem::transmute(gipa) };
            let inst = vk::Instance::from_raw((*vkctx).inst as u64);
            let instance = ash::Instance::load(&static_fn, inst);
            let dev = vk::Device::from_raw((*vkctx).act_dev as u64);
            let device = ash::Device::load(instance.fp_v1_0(), dev);
            let ext_sem = ash::khr::external_semaphore_win32::Device::new(&instance, &device);
            let phys = vk::PhysicalDevice::from_raw((*vkctx).phys_dev as u64);
            let mem_props = instance.get_physical_device_memory_properties(phys);
            // a queue family FFmpeg lets us use for transfers: transfer first, then compute, then graphics
            let nb = (*vkctx).nb_qf.max(0) as usize;
            let pick = |want: u32| (0..nb).find(|&i| ((*vkctx).qf[i].flags as u32 & want) != 0 && (*vkctx).qf[i].num > 0).map(|i| (*vkctx).qf[i].idx as u32);
            let qf = pick(vk::QueueFlags::TRANSFER.as_raw()).or_else(|| pick(vk::QueueFlags::COMPUTE.as_raw())).or_else(|| pick(vk::QueueFlags::GRAPHICS.as_raw()))?;
            let queue = device.get_device_queue(qf, 0);
            let pool = device.create_command_pool(&vk::CommandPoolCreateInfo::default().queue_family_index(qf).flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER), None).ok()?;
            let cmds = device.allocate_command_buffers(&vk::CommandBufferAllocateInfo::default().command_pool(pool).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(RING as u32)).ok()?;
            // the shared fence ↔ timeline semaphore
            let d3d: ID3D12Device = ID3D12Device::from_raw_borrowed(&d3d_raw)?.clone();
            let fence: ID3D12Fence = d3d.CreateFence(0, D3D12_FENCE_FLAG_SHARED).ok()?;
            let fh = d3d.CreateSharedHandle(&fence, None, 0x1000_0000u32 /* GENERIC_ALL */, windows::core::PCWSTR::null()).ok()?;
            let mut ty = vk::SemaphoreTypeCreateInfo::default().semaphore_type(vk::SemaphoreType::TIMELINE).initial_value(0);
            let sem = device.create_semaphore(&vk::SemaphoreCreateInfo::default().push_next(&mut ty), None).ok()?;
            ext_sem.import_semaphore_win32_handle(&vk::ImportSemaphoreWin32HandleInfoKHR::default().semaphore(sem).handle_type(vk::ExternalSemaphoreHandleTypeFlags::D3D12_FENCE).handle(fh.0 as isize)).ok()?;
            let fence_raw = fence.as_raw();
            let slots = cmds.into_iter().map(|cmd| Slot { planes: vec![], key: (0, 0, 0, 0), busy_until: 0, keep: None, cmd }).collect();
            log::info!("vk bridge: up on queue family {qf} — {RING}-slot ring of shared D3D12 plane textures, one shared fence");
            Some(VkBridge { instance, device, ext_sem, hwdev, vkctx, qf, queue, pool, mem_props, d3d, fence, fence_raw, sem, fence_value: 0, slots, next: 0, logged: false, _hw: hw.clone() })
        }
    }

    unsafe fn layout(&self, frame: *const ff::AVFrame) -> Option<Layout> { unsafe {
        let f = &*frame;
        if f.hw_frames_ctx.is_null() { return None }
        let fc = &*((*f.hw_frames_ctx).data as *const ff::AVHWFramesContext);
        let vkfc = &*(fc.hwctx as *const ff::AVVulkanFramesContext);
        let vkf = &*(f.data[0] as *const ff::AVVkFrame);
        let n_img = vkf.img.iter().take_while(|i| **i != 0).count();
        let desc = ff::av_pix_fmt_desc_get(fc.sw_format);
        if desc.is_null() || n_img == 0 { return None }
        let d = &*desc;
        let (w, h) = (f.width as u32, f.height as u32);
        let (cw, ch) = (w >> d.log2_chroma_w, h >> d.log2_chroma_h);
        let depth = d.comp[0].depth as u8;
        let nb = d.nb_components as usize;
        let fmt0 = vk::Format::from_raw(vkfc.format[0] as i32);
        let multiplane_2 = n_img == 1 && two_plane(fmt0);
        // (width, height, two channels) per destination plane
        let planes: Vec<(u32, u32, bool)> = if n_img == 1 {
            if multiplane_2 { vec![(w, h, false), (cw, ch, true)] }
            else { (0..nb).map(|i| if i == 0 || i == 3 { (w, h, false) } else { (cw, ch, false) }).collect() }
        } else { (0..n_img).map(|i| if i == 0 || i == 3 { (w, h, false) } else { (cw, ch, false) }).collect() };
        Some(Layout { n_img, multiplane_2, planes, depth, sw_format: fc.sw_format, w, h })
    } }

    unsafe fn make_plane(&self, w: u32, h: u32, two: bool, bits16: bool) -> Option<PlaneRes> { unsafe {
        let dxgi = match (two, bits16) { (false, false) => DXGI_FORMAT_R8_UNORM, (true, false) => DXGI_FORMAT_R8G8_UNORM, (false, true) => DXGI_FORMAT_R16_UNORM, (true, true) => DXGI_FORMAT_R16G16_UNORM };
        let vkfmt = match (two, bits16) { (false, false) => vk::Format::R8_UNORM, (true, false) => vk::Format::R8G8_UNORM, (false, true) => vk::Format::R16_UNORM, (true, true) => vk::Format::R16G16_UNORM };
        let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
        let desc = D3D12_RESOURCE_DESC { Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D, Alignment: 0, Width: w as u64, Height: h, DepthOrArraySize: 1, MipLevels: 1, Format: dxgi, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 }, Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN, Flags: D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS };
        let mut res: Option<ID3D12Resource> = None;
        self.d3d.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_SHARED, &desc, D3D12_RESOURCE_STATE_COMMON, None, &mut res).ok()?;
        let res = res?;
        let handle = self.d3d.CreateSharedHandle(&res, None, 0x1000_0000u32 /* GENERIC_ALL */, windows::core::PCWSTR::null()).ok()?;
        let mut ext = vk::ExternalMemoryImageCreateInfo::default().handle_types(vk::ExternalMemoryHandleTypeFlags::D3D12_RESOURCE);
        let info = vk::ImageCreateInfo::default().image_type(vk::ImageType::TYPE_2D).format(vkfmt).extent(vk::Extent3D { width: w, height: h, depth: 1 }).mip_levels(1).array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1).tiling(vk::ImageTiling::OPTIMAL).usage(vk::ImageUsageFlags::TRANSFER_DST).sharing_mode(vk::SharingMode::EXCLUSIVE).initial_layout(vk::ImageLayout::UNDEFINED).push_next(&mut ext);
        let image = self.device.create_image(&info, None).ok()?;
        let req = self.device.get_image_memory_requirements(image);
        let type_index = (0..self.mem_props.memory_type_count).find(|&i| (req.memory_type_bits & (1 << i)) != 0 && self.mem_props.memory_types[i as usize].property_flags.contains(vk::MemoryPropertyFlags::DEVICE_LOCAL))
            .or_else(|| (0..self.mem_props.memory_type_count).find(|&i| (req.memory_type_bits & (1 << i)) != 0))?;
        let mut import = vk::ImportMemoryWin32HandleInfoKHR::default().handle_type(vk::ExternalMemoryHandleTypeFlags::D3D12_RESOURCE).handle(handle.0 as isize);
        let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
        let alloc = vk::MemoryAllocateInfo::default().allocation_size(req.size).memory_type_index(type_index).push_next(&mut import).push_next(&mut dedicated);
        let memory = match self.device.allocate_memory(&alloc, None) { Ok(m) => m, Err(e) => { log::warn!("vk bridge: import of a D3D12 plane failed: {e:?}"); self.device.destroy_image(image, None); return None } };
        if self.device.bind_image_memory(image, memory, 0).is_err() { self.device.free_memory(memory, None); self.device.destroy_image(image, None); return None }
        let raw = res.as_raw();
        Some(PlaneRes { res, raw, image, memory, w, h, _handle: handle })
    } }

    /// Copy one decoded Vulkan frame into the ring and hand back the D3D12 plane set the presenter imports. `keep` is the
    /// AVFrame reference the slot holds until the copy has completed and the slot is reused. None = fall back to readback.
    pub unsafe fn export(&mut self, frame: *mut ff::AVFrame, keep: HwFrame) -> Option<Box<Dx12FrameHandle>> { unsafe {
        let lay = self.layout(frame)?;
        if !self.logged { log::info!("vk bridge: {}x{} {} — {} VkImage(s){}, {} destination plane(s), {}-bit", lay.w, lay.h, std::ffi::CStr::from_ptr(ff::av_get_pix_fmt_name(lay.sw_format)).to_string_lossy(), lay.n_img, if lay.multiplane_2 { " (2-plane multiplane)" } else if lay.n_img == 1 { " (multiplane)" } else { "" }, lay.planes.len(), lay.depth); self.logged = true; }
        let dev = self.device.clone();
        let idx = self.next; self.next = (self.next + 1) % RING;
        // the slot's previous copy must have completed before its textures and its AVFrame are reused
        if self.slots[idx].busy_until > 0 { let v = [self.slots[idx].busy_until]; let s = [self.sem]; let _ = dev.wait_semaphores(&vk::SemaphoreWaitInfo::default().semaphores(&s).values(&v), u64::MAX); }
        self.slots[idx].keep = None;
        let key = (lay.sw_format as i32, lay.w, lay.h, lay.planes.len() as u8);
        if self.slots[idx].key != key || self.slots[idx].planes.len() != lay.planes.len() {
            for p in self.slots[idx].planes.drain(..) { dev.destroy_image(p.image, None); dev.free_memory(p.memory, None); drop(p.res); }
            let mut planes = Vec::with_capacity(lay.planes.len());
            for &(w, h, two) in &lay.planes { planes.push(self.make_plane(w, h, two, lay.depth > 8)?); }
            self.slots[idx].planes = planes; self.slots[idx].key = key;
        }
        let f = &*frame;
        let vkf = &mut *(f.data[0] as *mut ff::AVVkFrame);
        let cmd = self.slots[idx].cmd;
        dev.begin_command_buffer(cmd, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)).ok()?;
        let full = |aspect: vk::ImageAspectFlags| vk::ImageSubresourceRange { aspect_mask: aspect, base_mip_level: 0, level_count: 1, base_array_layer: 0, layer_count: 1 };
        // sources → TRANSFER_SRC; destinations → TRANSFER_DST
        let mut pre: Vec<vk::ImageMemoryBarrier> = vec![];
        for i in 0..lay.n_img {
            pre.push(vk::ImageMemoryBarrier::default().old_layout(vk::ImageLayout::from_raw(vkf.layout[i] as i32)).new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                .src_access_mask(vk::AccessFlags::from_raw((vkf.access[i] as u64 & 0xffff_ffff) as u32)).dst_access_mask(vk::AccessFlags::TRANSFER_READ)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED).dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED).image(vk::Image::from_raw(vkf.img[i])).subresource_range(full(vk::ImageAspectFlags::COLOR)));
        }
        for p in &self.slots[idx].planes {
            pre.push(vk::ImageMemoryBarrier::default().old_layout(vk::ImageLayout::UNDEFINED).new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL).src_access_mask(vk::AccessFlags::empty()).dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED).dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED).image(p.image).subresource_range(full(vk::ImageAspectFlags::COLOR)));
        }
        dev.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::ALL_COMMANDS, vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), &[], &[], &pre);
        // the copies
        for (pi, p) in self.slots[idx].planes.iter().enumerate() {
            let (src_img, src_aspect) = if lay.n_img == 1 { (vk::Image::from_raw(vkf.img[0]), if lay.planes.len() == 1 { vk::ImageAspectFlags::COLOR } else { match pi { 0 => vk::ImageAspectFlags::PLANE_0, 1 => vk::ImageAspectFlags::PLANE_1, _ => vk::ImageAspectFlags::PLANE_2 } }) } else { (vk::Image::from_raw(vkf.img[pi]), vk::ImageAspectFlags::COLOR) };
            let region = vk::ImageCopy { src_subresource: vk::ImageSubresourceLayers { aspect_mask: src_aspect, mip_level: 0, base_array_layer: 0, layer_count: 1 }, src_offset: vk::Offset3D::default(),
                dst_subresource: vk::ImageSubresourceLayers { aspect_mask: vk::ImageAspectFlags::COLOR, mip_level: 0, base_array_layer: 0, layer_count: 1 }, dst_offset: vk::Offset3D::default(), extent: vk::Extent3D { width: p.w, height: p.h, depth: 1 } };
            dev.cmd_copy_image(cmd, src_img, vk::ImageLayout::TRANSFER_SRC_OPTIMAL, p.image, vk::ImageLayout::TRANSFER_DST_OPTIMAL, &[region]);
        }
        // sources back to FFmpeg's layout; destinations → GENERAL for the D3D12 side
        let mut post: Vec<vk::ImageMemoryBarrier> = vec![];
        for i in 0..lay.n_img {
            post.push(vk::ImageMemoryBarrier::default().old_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL).new_layout(vk::ImageLayout::from_raw(vkf.layout[i] as i32))
                .src_access_mask(vk::AccessFlags::TRANSFER_READ).dst_access_mask(vk::AccessFlags::from_raw((vkf.access[i] as u64 & 0xffff_ffff) as u32))
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED).dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED).image(vk::Image::from_raw(vkf.img[i])).subresource_range(full(vk::ImageAspectFlags::COLOR)));
        }
        for p in &self.slots[idx].planes {
            post.push(vk::ImageMemoryBarrier::default().old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL).new_layout(vk::ImageLayout::GENERAL).src_access_mask(vk::AccessFlags::TRANSFER_WRITE).dst_access_mask(vk::AccessFlags::MEMORY_READ)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED).dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED).image(p.image).subresource_range(full(vk::ImageAspectFlags::COLOR)));
        }
        dev.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::ALL_COMMANDS, vk::DependencyFlags::empty(), &[], &[], &post);
        dev.end_command_buffer(cmd).ok()?;
        // the submit: wait on the frame's semaphores at their values, signal them + the shared one
        let next_value = self.fence_value + 1;
        let mut waits: Vec<vk::Semaphore> = vec![]; let mut wait_vals: Vec<u64> = vec![]; let mut stages: Vec<vk::PipelineStageFlags> = vec![];
        let mut sigs: Vec<vk::Semaphore> = vec![]; let mut sig_vals: Vec<u64> = vec![];
        for i in 0..lay.n_img { if vkf.sem[i] != 0 { waits.push(vk::Semaphore::from_raw(vkf.sem[i])); wait_vals.push(vkf.sem_value[i]); stages.push(vk::PipelineStageFlags::TRANSFER); sigs.push(vk::Semaphore::from_raw(vkf.sem[i])); sig_vals.push(vkf.sem_value[i] + 1); } }
        sigs.push(self.sem); sig_vals.push(next_value);
        let mut tl = vk::TimelineSemaphoreSubmitInfo::default().wait_semaphore_values(&wait_vals).signal_semaphore_values(&sig_vals);
        let cmds = [cmd];
        let submit = vk::SubmitInfo::default().wait_semaphores(&waits).wait_dst_stage_mask(&stages).command_buffers(&cmds).signal_semaphores(&sigs).push_next(&mut tl);
        if let Some(lock) = (*self.vkctx).lock_queue { lock(self.hwdev, self.qf, 0); }
        let r = dev.queue_submit(self.queue, &[submit], vk::Fence::null());
        if let Some(unlock) = (*self.vkctx).unlock_queue { unlock(self.hwdev, self.qf, 0); }
        if let Err(e) = r { log::warn!("vk bridge: queue submit failed: {e:?}"); return None }
        for i in 0..lay.n_img { if vkf.sem[i] != 0 { vkf.sem_value[i] += 1; } }
        self.fence_value = next_value;
        self.slots[idx].busy_until = next_value; self.slots[idx].keep = Some(keep);
        let mut planes = [std::ptr::null_mut(); 4];
        for (i, p) in self.slots[idx].planes.iter().enumerate().take(4) { planes[i] = p.raw; }
        Some(Box::new(Dx12FrameHandle { resource: std::ptr::null_mut(), fence: self.fence_raw, fence_value: next_value, planes, plane_count: self.slots[idx].planes.len() as u8, interleaved_chroma: lay.multiplane_2, code_bits: lay.depth }))
    } }
}

impl Drop for VkBridge {
    fn drop(&mut self) {
        unsafe {
            // every copy must have landed before the images go (the last slot's value is the newest)
            if self.fence_value > 0 { let v = [self.fence_value]; let s = [self.sem]; let _ = self.device.wait_semaphores(&vk::SemaphoreWaitInfo::default().semaphores(&s).values(&v), 2_000_000_000); }
            for sl in self.slots.drain(..) { for p in sl.planes { self.device.destroy_image(p.image, None); self.device.free_memory(p.memory, None); drop(p.res); } drop(sl.keep); }
            self.device.destroy_semaphore(self.sem, None);
            self.device.destroy_command_pool(self.pool, None);
            let _ = (&self.instance, &self.ext_sem, &self.fence);
        }
    }
}
