use anyhow::Result;
use ash::vk;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::sync::Arc;

pub struct Surface {
    pub(crate) raw: vk::SurfaceKHR,
    pub(crate) fns: ash::khr::surface::Instance,
}

impl Surface {
    pub fn create<T>(instance: &super::instance::Instance, window: &T) -> Result<Arc<Self>>
    where
        T: HasDisplayHandle + HasWindowHandle,
    {
        let surface = unsafe {
            ash_window::create_surface(
                &instance.entry,
                &instance.raw,
                window.display_handle().unwrap().as_raw(),
                window.window_handle().unwrap().as_raw(),
                None,
            )?
        };
        let surface_loader = ash::khr::surface::Instance::new(&instance.entry, &instance.raw);

        Ok(Arc::new(Self {
            raw: surface,
            fns: surface_loader,
        }))
    }
}
