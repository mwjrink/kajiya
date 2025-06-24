use anyhow::Result;
use ash::vk;
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use std::{
    ffi::{CStr, CString, c_void},
    sync::Arc,
};

#[derive(Default)]
pub struct DeviceBuilder {
    pub required_extensions: Vec<*const i8>,
    pub graphics_debugging: bool,
}

impl DeviceBuilder {
    pub fn build(self) -> Result<Arc<Instance>> {
        Ok(Arc::new(Instance::create(self)?))
    }

    pub fn required_extensions(mut self, required_extensions: Vec<*const i8>) -> Self {
        self.required_extensions = required_extensions;
        self
    }

    pub fn graphics_debugging(mut self, graphics_debugging: bool) -> Self {
        self.graphics_debugging = graphics_debugging;
        self
    }
}

pub struct Instance {
    pub(crate) entry: ash::Entry,
    pub raw: ash::Instance,
    #[allow(dead_code)]
    pub(crate) debug_callback: Option<vk::DebugUtilsMessengerEXT>,
    #[allow(dead_code)]
    pub(crate) debug_loader: Option<ash::ext::debug_utils::Instance>,
    pub(crate) debug_utils: Option<ash::ext::debug_utils::Device>,
}

impl Instance {
    pub fn builder() -> DeviceBuilder {
        DeviceBuilder::default()
    }

    fn extension_names(builder: &DeviceBuilder) -> Vec<*const i8> {
        let mut names = vec![vk::KHR_GET_PHYSICAL_DEVICE_PROPERTIES2_NAME.as_ptr()];

        if builder.graphics_debugging {
            names.push(vk::EXT_DEBUG_REPORT_NAME.as_ptr());
            names.push(vk::EXT_DEBUG_UTILS_NAME.as_ptr());
        }

        names
    }

    fn layer_names(builder: &DeviceBuilder) -> Vec<CString> {
        let mut layer_names = Vec::new();
        if builder.graphics_debugging {
            layer_names.push(CString::new("VK_LAYER_KHRONOS_validation").unwrap());
        }
        layer_names
    }

    fn create(builder: DeviceBuilder) -> Result<Self> {
        let entry = unsafe { ash::Entry::load()? };
        let instance_extensions = builder
            .required_extensions
            .iter()
            .cloned()
            .chain(Self::extension_names(&builder).iter().cloned())
            .collect::<Vec<_>>();

        let layer_names = Self::layer_names(&builder);
        let layer_names: Vec<*const i8> = layer_names
            .iter()
            .map(|raw_name| raw_name.as_ptr())
            .collect();

        let app_desc = vk::ApplicationInfo::default().api_version(vk::make_api_version(0, 1, 2, 0));

        let instance_desc = vk::InstanceCreateInfo::default()
            .application_info(&app_desc)
            .enabled_layer_names(&layer_names)
            .enabled_extension_names(&instance_extensions);

        let instance = unsafe { entry.create_instance(&instance_desc, None)? };
        info!("Created a Vulkan instance");

        let (debug_loader, debug_callback, debug_utils) = if builder.graphics_debugging {
            let debug_info = vk::DebugUtilsMessengerCreateInfoEXT::default()
                .flags(vk::DebugUtilsMessengerCreateFlagsEXT::empty()) // reserved for future use
                .pfn_user_callback(Some(vulkan_debug_callback));

            #[allow(deprecated)]
            let debug_loader = ash::ext::debug_utils::Instance::new(&entry, &instance);

            let debug_callback = unsafe {
                #[allow(deprecated)]
                debug_loader
                    .create_debug_utils_messenger(&debug_info, None)
                    .unwrap()
            };

            // let debug_utils = ash::ext::debug_utils::Device::new(&instance, &device);

            (Some(debug_loader), Some(debug_callback), None) //Some(debug_utils))
        } else {
            (None, None, None)
        };

        Ok(Self {
            entry,
            raw: instance,
            debug_callback,
            debug_loader,
            debug_utils,
        })
    }
}

unsafe extern "system" fn vulkan_debug_callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    typ: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _p_user_data: *mut c_void,
) -> vk::Bool32 {
    unsafe {
        let message = CStr::from_ptr((*p_callback_data).p_message);

        // #[allow(clippy::if_same_then_else)]
        // if message.starts_with("Validation Error: [ VUID-VkWriteDescriptorSet-descriptorType-00322")
        //     || message
        //         .starts_with("Validation Error: [ VUID-VkWriteDescriptorSet-descriptorType-02752")
        // {
        //     // Validation layers incorrectly report an error in pushing immutable sampler descriptors.
        //     //
        //     // https://www.khronos.org/registry/vulkan/specs/1.2-extensions/man/html/vkCmdPushDescriptorSetKHR.html
        //     // This documentation claims that it's necessary to push immutable samplers.
        // } else if message.starts_with("Validation Performance Warning") {
        // } else if message.starts_with("Validation Warning: [ VUID_Undefined ]") {
        //     log::warn!("{}\n", message);
        // } else {
        //     log::error!("{}\n", message);
        // }

        if severity == vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE {
            log::trace!(target: "nlvr::rendering::vulkan::validation", "{:?} - {:?}", typ, message);
        } else if severity == vk::DebugUtilsMessageSeverityFlagsEXT::INFO {
            log::info!(target: "nlvr::rendering::vulkan::validation", "{:?} - {:?}", typ, message);
        } else if severity == vk::DebugUtilsMessageSeverityFlagsEXT::WARNING {
            log::warn!(target: "nlvr::rendering::vulkan::validation", "{:?} - {:?}", typ, message);
        } else if severity == vk::DebugUtilsMessageSeverityFlagsEXT::ERROR {
            log::error!(target: "nlvr::rendering::vulkan::validation", "{:?} - {:?}", typ, message);
        } else {
            log::debug!(target: "nlvr::rendering::vulkan::validation", "UNKNOWN VULKAN VALIDATION LAYER LOG: {:?} - {:?}", typ, message);
        }
    }
    vk::FALSE
}
