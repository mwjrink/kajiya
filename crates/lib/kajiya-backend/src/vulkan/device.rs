use crate::{BackendError, vulkan::buffer::BufferDesc};

pub use super::profiler::VkProfilerData;
use super::{
    buffer::Buffer,
    error::CrashMarkerNames,
    physical_device::{PhysicalDevice, QueueFamily},
    profiler::ProfilerBackend,
};
use anyhow::Result;
use ash::vk;
use gpu_allocator::{
    AllocatorDebugSettings,
    vulkan::{Allocator, AllocatorCreateDesc},
};
use gpu_profiler::backend::ash::VulkanProfilerFrame;
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use parking_lot::Mutex;
use std::{
    collections::{HashMap, HashSet},
    os::raw::c_char,
    sync::Arc,
};

/// Descriptor count to subtract from the max bindless descriptor count,
/// so that we don't overflow the max when using bindless _and_ non-bindless descriptors
/// in the same shader stage.
pub const RESERVED_DESCRIPTOR_COUNT: u32 = 32;

pub struct Queue {
    pub raw: vk::Queue,
    pub family: QueueFamily,
}

pub trait DeferredRelease: Copy {
    fn enqueue_release(self, pending: &mut PendingResourceReleases);
}

impl DeferredRelease for vk::DescriptorPool {
    fn enqueue_release(self, pending: &mut PendingResourceReleases) {
        pending.descriptor_pools.push(self);
    }
}

#[derive(Default)]
pub struct PendingResourceReleases {
    pub descriptor_pools: Vec<vk::DescriptorPool>,
}

impl PendingResourceReleases {
    fn release_all(&mut self, device: &ash::Device) {
        unsafe {
            for res in self.descriptor_pools.drain(..) {
                device.destroy_descriptor_pool(res, None);
            }
        }
    }
}

pub struct DeviceFrame {
    //pub(crate) linear_allocator_pool: vk_mem::AllocatorPool,
    pub swapchain_acquired_semaphore: Option<vk::Semaphore>,
    pub rendering_complete_semaphore: Option<vk::Semaphore>,
    pub main_command_buffer: CommandBuffer,
    pub presentation_command_buffer: CommandBuffer,
    pub pending_resource_releases: Mutex<PendingResourceReleases>,
    pub profiler_data: VkProfilerData,
}

pub struct CommandBuffer {
    pub raw: vk::CommandBuffer,
    pub submit_done_fence: vk::Fence,
    //pool: vk::CommandPool,
}

impl CommandBuffer {
    fn new(device: &ash::Device, queue_family: &QueueFamily) -> Result<Self> {
        let pool_create_info = vk::CommandPoolCreateInfo::default()
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
            .queue_family_index(queue_family.index);

        let pool = unsafe { device.create_command_pool(&pool_create_info, None).unwrap() };

        let command_buffer_allocate_info = vk::CommandBufferAllocateInfo::default()
            .command_buffer_count(1)
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY);

        let cb = unsafe {
            device
                .allocate_command_buffers(&command_buffer_allocate_info)
                .unwrap()
        }[0];

        let submit_done_fence = unsafe {
            device.create_fence(
                &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                None,
            )
        }?;

        Ok(CommandBuffer {
            raw: cb,
            //pool,
            submit_done_fence,
        })
    }
}

impl DeviceFrame {
    pub fn new(
        pdevice: &PhysicalDevice,
        device: &ash::Device,
        global_allocator: &mut Allocator,
        queue_family: &QueueFamily,
    ) -> Self {
        Self {
            /*linear_allocator_pool: global_allocator
            .create_pool(&{
                let mut info = vk_mem::AllocatorPoolCreateInfo::default();
                info.flags = vk_mem::AllocatorPoolCreateFlags::LINEAR_ALGORITHM;
                info
            })
            .expect("linear allocator"),*/
            swapchain_acquired_semaphore: None,
            rendering_complete_semaphore: None,
            main_command_buffer: CommandBuffer::new(device, queue_family).unwrap(),
            presentation_command_buffer: CommandBuffer::new(device, queue_family).unwrap(),
            pending_resource_releases: Default::default(),
            profiler_data: VulkanProfilerFrame::new(
                device,
                ProfilerBackend::new(
                    device,
                    global_allocator,
                    pdevice.properties.limits.timestamp_period,
                ),
            ),
        }
    }
}

pub struct Device {
    pub raw: ash::Device,
    pub(crate) pdevice: Arc<PhysicalDevice>,
    pub(crate) instance: Arc<super::instance::Instance>,
    pub universal_queue: Queue,
    pub(crate) global_allocator: Arc<Mutex<Allocator>>,
    pub(crate) immutable_samplers: HashMap<SamplerDesc, vk::Sampler>,
    pub(crate) setup_cb: Mutex<CommandBuffer>,

    pub(crate) crash_tracking_buffer: Buffer,
    pub(crate) crash_marker_names: Mutex<CrashMarkerNames>,

    pub acceleration_structure_ext: ash::khr::acceleration_structure::Device,
    pub ray_tracing_pipeline_ext: ash::khr::ray_tracing_pipeline::Device,
    // pub ray_query_ext: khr::RayQuery,
    pub ray_tracing_pipeline_properties: vk::PhysicalDeviceRayTracingPipelinePropertiesKHR<'static>,

    frames: [Mutex<Arc<DeviceFrame>>; 2],

    ray_tracing_enabled: bool,
}

// Allowing `Send` on `frames` is technically unsound. There are some checks
// in place that `Arc<DeviceFrame>` doesn't get retained by the user,
// but it begs for a clearer solution.
#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl Send for Device {}

unsafe impl Sync for Device {}

impl Device {
    pub fn create(pdevice: &Arc<PhysicalDevice>) -> Result<Arc<Self>> {
        let supported_extensions: HashSet<String> = unsafe {
            let extension_properties = pdevice
                .instance
                .raw
                .enumerate_device_extension_properties(pdevice.raw)?;
            debug!("Extension properties:\n{:#?}", &extension_properties);

            extension_properties
                .iter()
                .map(|ext| {
                    std::ffi::CStr::from_ptr(ext.extension_name.as_ptr() as *const c_char)
                        .to_string_lossy()
                        .as_ref()
                        .to_owned()
                })
                .collect()
        };

        let mut device_extension_names = vec![
            vk::EXT_DESCRIPTOR_INDEXING_NAME.as_ptr(),
            vk::EXT_SCALAR_BLOCK_LAYOUT_NAME.as_ptr(),
            vk::KHR_MAINTENANCE1_NAME.as_ptr(),
            vk::KHR_MAINTENANCE2_NAME.as_ptr(),
            vk::KHR_MAINTENANCE3_NAME.as_ptr(),
            vk::KHR_GET_MEMORY_REQUIREMENTS2_NAME.as_ptr(),
            vk::KHR_IMAGELESS_FRAMEBUFFER_NAME.as_ptr(),
            vk::KHR_IMAGE_FORMAT_LIST_NAME.as_ptr(),
            vk::KHR_DESCRIPTOR_UPDATE_TEMPLATE_NAME.as_ptr(),
            // Rust-GPU
            vk::KHR_SHADER_FLOAT16_INT8_NAME.as_ptr(),
            // DLSS
            #[cfg(feature = "dlss")]
            vk::NVX_BINARY_IMPORT_NAME.as_ptr(),
            #[cfg(feature = "dlss")]
            vk::KHR_PUSH_DESCRIPTOR_NAME.as_ptr(),
            #[cfg(feature = "dlss")]
            vk::NVX_IMAGE_VIEW_HANDLE_NAME.as_ptr(),
        ];

        let ray_tracing_extensions = [
            vk::KHR_VULKAN_MEMORY_MODEL_NAME.as_ptr(), // used in ray tracing shaders
            vk::KHR_PIPELINE_LIBRARY_NAME.as_ptr(),    // rt dep
            vk::KHR_DEFERRED_HOST_OPERATIONS_NAME.as_ptr(), // rt dep
            vk::EXT_BUFFER_DEVICE_ADDRESS_NAME.as_ptr(), // rt dep
            vk::KHR_ACCELERATION_STRUCTURE_NAME.as_ptr(),
            vk::KHR_RAY_TRACING_PIPELINE_NAME.as_ptr(),
        ];

        let ray_tracing_enabled = unsafe {
            ray_tracing_extensions.iter().all(|ext| {
                let ext = std::ffi::CStr::from_ptr(*ext).to_string_lossy();

                let supported = supported_extensions.contains(ext.as_ref());

                if !supported {
                    log::info!("Ray tracing extension not supported: {}", ext);
                }

                supported
            })
        };

        if ray_tracing_enabled {
            log::info!("All ray tracing extensions are supported");

            device_extension_names.extend(ray_tracing_extensions.iter());
        }

        if pdevice.presentation_requested {
            device_extension_names.push(vk::KHR_SWAPCHAIN_NAME.as_ptr());
        }

        unsafe {
            for &ext in &device_extension_names {
                let ext = std::ffi::CStr::from_ptr(ext).to_string_lossy();
                if !supported_extensions.contains(ext.as_ref()) {
                    panic!("Device extension not supported: {}", ext);
                }
            }
        }

        let priorities = [1.0];

        let universal_queue = pdevice
            .queue_families
            .iter()
            .filter(|qf| qf.properties.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            .copied()
            .next();

        let universal_queue = if let Some(universal_queue) = universal_queue {
            universal_queue
        } else {
            anyhow::bail!("No suitable render queue found");
        };

        let universal_queue_info = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(universal_queue.index)
            .queue_priorities(&priorities)];

        let mut scalar_block = vk::PhysicalDeviceScalarBlockLayoutFeaturesEXT::default();
        let mut descriptor_indexing = vk::PhysicalDeviceDescriptorIndexingFeaturesEXT::default();
        let mut imageless_framebuffer =
            vk::PhysicalDeviceImagelessFramebufferFeaturesKHR::default();
        let mut shader_float16_int8 = vk::PhysicalDeviceShaderFloat16Int8Features::default();
        let mut vulkan_memory_model = vk::PhysicalDeviceVulkanMemoryModelFeaturesKHR::default();
        let mut get_buffer_device_address_features =
            ash::vk::PhysicalDeviceBufferDeviceAddressFeatures::default();

        let mut acceleration_structure_features =
            ash::vk::PhysicalDeviceAccelerationStructureFeaturesKHR::default();

        let mut ray_tracing_pipeline_features =
            ash::vk::PhysicalDeviceRayTracingPipelineFeaturesKHR::default();

        unsafe {
            let instance = &pdevice.instance.raw;

            // this is really stupid but I have no idea why the compiler
            // complains about this here and not in other ash projects
            let mut features2 = vk::PhysicalDeviceFeatures2::default()
                .push_next((&raw mut scalar_block).clone().as_mut().unwrap())
                .push_next((&raw mut descriptor_indexing).clone().as_mut().unwrap())
                .push_next((&raw mut imageless_framebuffer).clone().as_mut().unwrap())
                .push_next((&raw mut shader_float16_int8).clone().as_mut().unwrap())
                .push_next((&raw mut vulkan_memory_model).clone().as_mut().unwrap())
                .push_next(
                    (&raw mut get_buffer_device_address_features)
                        .clone()
                        .as_mut()
                        .unwrap(),
                );

            if ray_tracing_enabled {
                features2 = features2
                    .push_next(
                        (&raw mut acceleration_structure_features)
                            .clone()
                            .as_mut()
                            .unwrap(),
                    )
                    .push_next(
                        (&raw mut ray_tracing_pipeline_features)
                            .clone()
                            .as_mut()
                            .unwrap(),
                    );
            }

            instance.get_physical_device_features2(pdevice.raw, &mut features2);

            debug!("{:#?}", &scalar_block);
            debug!("{:#?}", &descriptor_indexing);
            debug!("{:#?}", &imageless_framebuffer);
            debug!("{:#?}", &shader_float16_int8);
            debug!("{:#?}", &vulkan_memory_model);
            debug!("{:#?}", &get_buffer_device_address_features);

            // The suggested `#[rustfmt::skip]` is not stable
            #[allow(clippy::deprecated_cfg_attr)]
            #[cfg_attr(rustfmt, rustfmt_skip)]
            {
                assert_ne!(scalar_block.scalar_block_layout, 0);

                assert_ne!(descriptor_indexing.shader_uniform_texel_buffer_array_dynamic_indexing, 0);
                assert_ne!(descriptor_indexing.shader_storage_texel_buffer_array_dynamic_indexing, 0);
                assert_ne!(descriptor_indexing.shader_sampled_image_array_non_uniform_indexing, 0);
                assert_ne!(descriptor_indexing.shader_storage_image_array_non_uniform_indexing, 0);
                assert_ne!(descriptor_indexing.shader_uniform_texel_buffer_array_non_uniform_indexing, 0);
                assert_ne!(descriptor_indexing.shader_storage_texel_buffer_array_non_uniform_indexing, 0);
                assert_ne!(descriptor_indexing.descriptor_binding_sampled_image_update_after_bind, 0);
                assert_ne!(descriptor_indexing.descriptor_binding_update_unused_while_pending, 0);
                assert_ne!(descriptor_indexing.descriptor_binding_partially_bound, 0);
                assert_ne!(descriptor_indexing.descriptor_binding_variable_descriptor_count, 0);
                assert_ne!(descriptor_indexing.runtime_descriptor_array, 0);

                assert_ne!(imageless_framebuffer.imageless_framebuffer, 0);

                assert_ne!(shader_float16_int8.shader_int8, 0);

                if ray_tracing_enabled {
                    assert_ne!(descriptor_indexing.shader_uniform_buffer_array_non_uniform_indexing, 0);
                    assert_ne!(descriptor_indexing.shader_storage_buffer_array_non_uniform_indexing, 0);

                    assert_ne!(vulkan_memory_model.vulkan_memory_model, 0);

                    assert_ne!(acceleration_structure_features.acceleration_structure, 0);
                    assert_ne!(acceleration_structure_features.descriptor_binding_acceleration_structure_update_after_bind, 0);

                    assert_ne!(ray_tracing_pipeline_features.ray_tracing_pipeline, 0);
                    assert_ne!(ray_tracing_pipeline_features.ray_tracing_pipeline_trace_rays_indirect, 0);

                    assert_ne!(get_buffer_device_address_features.buffer_device_address, 0);
                }
            }

            let device_create_info = vk::DeviceCreateInfo::default()
                .queue_create_infos(&universal_queue_info)
                .enabled_extension_names(&device_extension_names)
                .push_next(&mut features2);

            let device = instance
                .create_device(pdevice.raw, &device_create_info, None)
                .unwrap();

            info!("Created a Vulkan device");

            let mut global_allocator = Allocator::new(&AllocatorCreateDesc {
                instance: instance.clone(),
                device: device.clone(),
                physical_device: pdevice.raw,
                debug_settings: AllocatorDebugSettings {
                    log_leaks_on_shutdown: false,
                    log_memory_information: true,
                    log_allocations: true,
                    ..Default::default()
                },
                buffer_device_address: true,
                allocation_sizes: Default::default(),
            })
            .unwrap();

            let universal_queue = Queue {
                raw: device.get_device_queue(universal_queue.index, 0),
                family: universal_queue,
            };

            let frame0 = DeviceFrame::new(
                pdevice,
                &device,
                &mut global_allocator,
                &universal_queue.family,
            );
            let frame1 = DeviceFrame::new(
                pdevice,
                &device,
                &mut global_allocator,
                &universal_queue.family,
            );
            //let frame2 = DeviceFrame::new(&device, &mut global_allocator, &universal_queue.family);

            let immutable_samplers = Self::create_samplers(&device);
            let setup_cb = CommandBuffer::new(&device, &universal_queue.family).unwrap();

            let acceleration_structure_ext =
                ash::khr::acceleration_structure::Device::new(&pdevice.instance.raw, &device);
            let ray_tracing_pipeline_ext =
                ash::khr::ray_tracing_pipeline::Device::new(&pdevice.instance.raw, &device);

            let mut physical_device_properties2 = vk::PhysicalDeviceProperties2::default();
            let mut ray_tracing_pipeline_properties =
                vk::PhysicalDeviceRayTracingPipelinePropertiesKHR::default();
            physical_device_properties2.push_next(&mut ray_tracing_pipeline_properties);
            instance.get_physical_device_properties2(pdevice.raw, &mut physical_device_properties2);

            let crash_tracking_buffer = Self::create_buffer_impl(
                &device,
                &mut global_allocator,
                BufferDesc::new_gpu_to_cpu(4, vk::BufferUsageFlags::TRANSFER_DST),
                "crash tracking buffer",
            )?;

            Ok(Arc::new(Device {
                pdevice: pdevice.clone(),
                instance: pdevice.instance.clone(),
                raw: device,
                universal_queue,
                global_allocator: Arc::new(Mutex::new(global_allocator)),
                immutable_samplers,
                setup_cb: Mutex::new(setup_cb),
                crash_tracking_buffer,
                crash_marker_names: Default::default(),
                acceleration_structure_ext,
                ray_tracing_pipeline_ext,
                // ray_query_ext,
                ray_tracing_pipeline_properties,
                frames: [
                    Mutex::new(Arc::new(frame0)),
                    Mutex::new(Arc::new(frame1)),
                    //Mutex::new(Arc::new(frame2)),
                ],
                ray_tracing_enabled,
            }))
        }
    }

    fn create_samplers(device: &ash::Device) -> HashMap<SamplerDesc, vk::Sampler> {
        let texel_filters = [vk::Filter::NEAREST, vk::Filter::LINEAR];
        let mipmap_modes = [
            vk::SamplerMipmapMode::NEAREST,
            vk::SamplerMipmapMode::LINEAR,
        ];
        let address_modes = [
            vk::SamplerAddressMode::REPEAT,
            vk::SamplerAddressMode::CLAMP_TO_EDGE,
        ];

        let mut result = HashMap::new();

        for &texel_filter in &texel_filters {
            for &mipmap_mode in &mipmap_modes {
                for &address_modes in &address_modes {
                    let anisotropy_enable = texel_filter == vk::Filter::LINEAR;

                    result.insert(
                        SamplerDesc {
                            texel_filter,
                            mipmap_mode,
                            address_modes,
                        },
                        unsafe {
                            device.create_sampler(
                                &vk::SamplerCreateInfo::default()
                                    .mag_filter(texel_filter)
                                    .min_filter(texel_filter)
                                    .mipmap_mode(mipmap_mode)
                                    .address_mode_u(address_modes)
                                    .address_mode_v(address_modes)
                                    .address_mode_w(address_modes)
                                    .max_lod(vk::LOD_CLAMP_NONE)
                                    .max_anisotropy(16.0)
                                    .anisotropy_enable(anisotropy_enable),
                                None,
                            )
                        }
                        .expect("create_sampler"),
                    );
                }
            }
        }

        result
    }

    pub fn get_sampler(&self, desc: SamplerDesc) -> vk::Sampler {
        *self
            .immutable_samplers
            .get(&desc)
            .unwrap_or_else(|| panic!("Sampler not found: {:?}", desc))
    }

    pub fn begin_frame(&self) -> Arc<DeviceFrame> {
        let mut frame0 = self.frames[0].lock();
        {
            let frame0: &mut DeviceFrame = Arc::get_mut(&mut frame0).unwrap_or_else(|| {
                panic!("Unable to begin frame: frame data is being held by user code")
            });

            // Wait for the the GPU to be done with the previously submitted frame,
            // so that we can access its data again.
            //
            // We can't use device.frame[0] before this, or we race with the GPU.
            //
            // TODO: the wait here protects more than the command buffers (such as dynamic constants),
            // but the fence belongs to command buffers, creating a confusing relationship.
            unsafe {
                puffin::profile_scope!("wait submit done");

                self.raw
                    .wait_for_fences(
                        // Note: need to wait for both command buffers so that the GPU won't
                        // be accessing frame[0] any more after this.
                        &[
                            frame0.main_command_buffer.submit_done_fence,
                            frame0.presentation_command_buffer.submit_done_fence,
                        ],
                        true,
                        std::u64::MAX,
                    )
                    .map_err(|err| self.report_error(err.into()))
                    .expect("Wait for fence failed.");
            }

            puffin::profile_scope!("release pending resources");
            frame0
                .pending_resource_releases
                .get_mut()
                .release_all(&self.raw);
        }

        frame0.clone()
    }

    pub fn defer_release(&self, resource: impl DeferredRelease) {
        resource.enqueue_release(&mut self.frames[0].lock().pending_resource_releases.lock());
    }

    pub fn with_setup_cb(
        &self,
        callback: impl FnOnce(vk::CommandBuffer),
    ) -> Result<(), BackendError> {
        let cb = self.setup_cb.lock();

        unsafe {
            self.raw
                .begin_command_buffer(
                    cb.raw,
                    &vk::CommandBufferBeginInfo::default()
                        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
                )
                .unwrap();
        }

        callback(cb.raw);

        unsafe {
            self.raw.end_command_buffer(cb.raw).unwrap();

            let submit_info =
                vk::SubmitInfo::default().command_buffers(std::slice::from_ref(&cb.raw));

            self.raw
                .queue_submit(self.universal_queue.raw, &[submit_info], vk::Fence::null())
                .expect("queue submit failed.");

            log::trace!("device_wait_idle");

            Ok(self.raw.device_wait_idle()?)
        }
    }

    pub fn finish_frame(&self, frame: Arc<DeviceFrame>) {
        drop(frame);

        let mut frame0 = self.frames[0].lock();
        let frame0: &mut DeviceFrame = Arc::get_mut(&mut frame0).unwrap_or_else(|| {
            panic!("Unable to finish frame: frame data is being held by user code")
        });

        {
            let mut frame1 = self.frames[1].lock();
            let frame1: &mut DeviceFrame = Arc::get_mut(&mut frame1).unwrap();

            //let mut frame2 = self.frames[2].lock();
            //let frame2: &mut DeviceFrame = Arc::get_mut(&mut frame2).unwrap();

            std::mem::swap(frame0, frame1);
            //std::mem::swap(frame1, frame2);
        }
    }

    pub fn physical_device(&self) -> &PhysicalDevice {
        self.pdevice.as_ref()
    }

    pub fn debug_utils(&self) -> Option<&ash::ext::debug_utils::Device> {
        self.instance.debug_utils.as_ref()
    }

    pub fn max_bindless_descriptor_count(&self) -> u32 {
        (512 * 1024).min(
            self.pdevice
                .properties
                .limits
                .max_per_stage_descriptor_sampled_images
                - RESERVED_DESCRIPTOR_COUNT,
        )
    }

    pub fn ray_tracing_enabled(&self) -> bool {
        self.ray_tracing_enabled
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            log::trace!("device_wait_idle");
            let _ = self.raw.device_wait_idle();
        }
    }
}

// TODO
/*impl Drop for Device {
    fn drop(&mut self) {
        let mut frame0 = self.frame0.lock();
        let mut frame1 = self.frame1.lock();

        let frame0: &mut DeviceFrame = Arc::get_mut(&mut frame0).unwrap_or_else(|| {
            panic!("Unable to deallocate DeviceFrame: frame data is being held by user code")
        });
        let frame1: &mut DeviceFrame = Arc::get_mut(&mut frame1).unwrap_or_else(|| {
            panic!("Unable to deallocate DeviceFrame: frame data is being held by user code(2)")
        });

        /*self.global_allocator
            .destroy_pool(&frame0.linear_allocator_pool)
            .unwrap();

        self.global_allocator
            .destroy_pool(&frame1.linear_allocator_pool)
            .unwrap();*/
    }
}*/

#[derive(Clone, Copy, Hash, PartialEq, Eq, Debug)]
pub struct SamplerDesc {
    pub texel_filter: vk::Filter,
    pub mipmap_mode: vk::SamplerMipmapMode,
    pub address_modes: vk::SamplerAddressMode,
}
