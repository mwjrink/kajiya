use std::borrow::Cow;

use kajiya::{
    RenderOverrideFlags, backend::gpu_profiler, math::Vec3, world_renderer::RenderDebugMode,
};
use kajiya_simple::main_loop::FrameContext;

use crate::{
    PersistedState,
    runtime::{MAX_FPS_LIMIT, RuntimeState},
};

impl RuntimeState {
    pub fn do_gui(&mut self, persisted: &mut PersistedState, ctx: &mut FrameContext) {
        if self.keyboard.was_just_pressed(self.keymap_config.ui.toggle) {
            self.show_gui = !self.show_gui;
        }

        ctx.world_renderer.rg_debug_hook = self.locked_rg_debug_hook.clone();

        if self.show_gui {
            ctx.imgui.take().unwrap().frame(|ui| {
                let ui = ui.new_frame();
                if imgui::CollapsingHeader::new("Tweaks")
                    .default_open(true)
                    .build(ui)
                {
                    imgui::Drag::<f32, &str>::new("EV shift")
                        .range(-8.0, 12.0)
                        .speed(0.01)
                        .build(ui, &mut persisted.exposure.ev_shift);

                    ui.checkbox(
                        "Use dynamic exposure",
                        &mut persisted.exposure.use_dynamic_adaptation,
                    );

                    imgui::Drag::<f32, &str>::new("Adaptation speed")
                        .range(-4.0, 4.0)
                        .speed(0.01)
                        .build(ui, &mut persisted.exposure.dynamic_adaptation_speed);

                    imgui::Drag::<f32, &str>::new("Luminance histogram low clip")
                        .range(0.0, 1.0)
                        .speed(0.001)
                        .build(ui, &mut persisted.exposure.dynamic_adaptation_low_clip);
                    persisted.exposure.dynamic_adaptation_low_clip = persisted
                        .exposure
                        .dynamic_adaptation_low_clip
                        .clamp(0.0, 1.0);

                    imgui::Drag::<f32, &str>::new("Luminance histogram high clip")
                        .range(0.0, 1.0)
                        .speed(0.001)
                        .build(ui, &mut persisted.exposure.dynamic_adaptation_high_clip);
                    persisted.exposure.dynamic_adaptation_high_clip = persisted
                        .exposure
                        .dynamic_adaptation_high_clip
                        .clamp(0.0, 1.0);

                    imgui::Drag::<f32, &str>::new("Contrast")
                        .range(1.0, 1.5)
                        .speed(0.001)
                        .build(ui, &mut persisted.exposure.contrast);

                    imgui::Drag::<f32, &str>::new("Emissive multiplier")
                        .range(0.0, 10.0)
                        .speed(0.1)
                        .build(ui, &mut persisted.light.emissive_multiplier);

                    ui.checkbox("Enable emissive", &mut persisted.light.enable_emissive);

                    imgui::Drag::<f32, &str>::new("Light intensity multiplier")
                        .range(0.0, 1000.0)
                        .speed(1.0)
                        .build(ui, &mut persisted.light.local_lights.multiplier);

                    imgui::Drag::<f32, &str>::new("Camera speed")
                        .range(0.0, 10.0)
                        .speed(0.025)
                        .build(ui, &mut persisted.movement.camera_speed);

                    imgui::Drag::<f32, &str>::new("Camera smoothness")
                        .range(0.0, 20.0)
                        .speed(0.1)
                        .build(ui, &mut persisted.movement.camera_smoothness);

                    imgui::Drag::<f32, &str>::new("Sun rotation smoothness")
                        .range(0.0, 20.0)
                        .speed(0.1)
                        .build(ui, &mut persisted.movement.sun_rotation_smoothness);

                    imgui::Drag::<f32, &str>::new("Field of view")
                        .range(1.0, 120.0)
                        .speed(0.25)
                        .build(ui, &mut persisted.camera.vertical_fov);

                    imgui::Drag::<f32, &str>::new("Sun size")
                        .range(0.0, 10.0)
                        .speed(0.02)
                        .build(ui, &mut persisted.light.sun.size_multiplier);

                    /*ui.checkbox(
                        "Show world radiance cache"),
                        &mut ctx.world_renderer.debug_show_wrc,
                    );*/

                    /*if ui.radio_button_bool(
                        "Move sun"),
                        left_click_edit_mode == LeftClickEditMode::MoveSun,
                    ) {
                        left_click_edit_mode = LeftClickEditMode::MoveSun;
                    }

                    if ui.radio_button_bool(
                        "Move local lights"),
                        left_click_edit_mode == LeftClickEditMode::MoveLocalLights,
                    ) {
                        left_click_edit_mode = LeftClickEditMode::MoveLocalLights;
                    }

                    imgui::Drag::<u32>::new("Light count"))
                        .range(0..=10)
                        .build(ui, &mut state.lights.count);*/

                    ui.checkbox(
                        "Scroll irradiance cache",
                        &mut ctx.world_renderer.ircache.enable_scroll,
                    );

                    imgui::Drag::<u32>::new("GI spatial reuse passes")
                        .range(1, 3)
                        .build(ui, &mut ctx.world_renderer.rtdgi.spatial_reuse_pass_count);

                    ctx.world_renderer.rtdgi.spatial_reuse_pass_count = ctx
                        .world_renderer
                        .rtdgi
                        .spatial_reuse_pass_count
                        .clamp(1, 3);

                    ui.checkbox(
                        "Ray-traced reservoir visibility",
                        &mut ctx.world_renderer.rtdgi.use_raytraced_reservoir_visibility,
                    );

                    ui.checkbox(
                        "Allow diffuse ray reuse for reflections",
                        &mut ctx.world_renderer.rtr.reuse_rtdgi_rays,
                    );

                    #[cfg(feature = "dlss")]
                    {
                        ui.checkbox("Use DLSS", &mut ctx.world_renderer.use_dlss);
                    }
                }

                if imgui::CollapsingHeader::new("Scene")
                    .default_open(true)
                    .build(ui)
                {
                    if let Some(ibl) = persisted.scene.ibl.as_ref() {
                        ui.text(format!("IBL: {:?}", ibl));
                        if ui.button("Unload") {
                            ctx.world_renderer.ibl.unload_image();
                            persisted.scene.ibl = None;
                        }
                    } else {
                        ui.text("Drag a sphere-mapped .hdr/.exr to load as IBL");
                    }

                    let mut element_to_remove = None;
                    for (idx, elem) in persisted.scene.elements.iter_mut().enumerate() {
                        ui.dummy([0.0, 10.0]);

                        let id_token = ui.push_id(format!("{}", idx as i32));
                        ui.text(format!("{:?}", elem.source));

                        {
                            ui.set_next_item_width(200.0);

                            let mut scale = elem.transform.scale.x;
                            imgui::Drag::<f32, &str>::new("scale")
                                .range(0.001, 1000.0)
                                .speed(1.0)
                                .flags(imgui::SliderFlags::LOGARITHMIC)
                                .build(ui, &mut scale);
                            if scale != elem.transform.scale.x {
                                elem.transform.scale = Vec3::splat(scale);
                            }
                        }

                        ui.same_line();
                        if ui.button("Delete") {
                            element_to_remove = Some(idx);
                        }

                        // Position
                        {
                            ui.set_next_item_width(100.0);
                            imgui::Drag::<f32, &str>::new("x")
                                .speed(0.01)
                                .build(ui, &mut elem.transform.position.x);

                            ui.same_line();

                            ui.set_next_item_width(100.0);
                            imgui::Drag::<f32, &str>::new("y")
                                .speed(0.01)
                                .build(ui, &mut elem.transform.position.y);

                            ui.same_line();

                            ui.set_next_item_width(100.0);
                            imgui::Drag::<f32, &str>::new("z")
                                .speed(0.01)
                                .build(ui, &mut elem.transform.position.z);
                        }

                        // Rotation
                        {
                            ui.set_next_item_width(100.0);
                            imgui::Drag::<f32, &str>::new("rx")
                                .speed(0.1)
                                .build(ui, &mut elem.transform.rotation_euler_degrees.x);

                            ui.same_line();

                            ui.set_next_item_width(100.0);
                            imgui::Drag::<f32, &str>::new("ry")
                                .speed(0.1)
                                .build(ui, &mut elem.transform.rotation_euler_degrees.y);

                            ui.same_line();

                            ui.set_next_item_width(100.0);
                            imgui::Drag::<f32, &str>::new("rz")
                                .speed(0.1)
                                .build(ui, &mut elem.transform.rotation_euler_degrees.z);
                        }

                        id_token.pop();
                    }

                    if let Some(idx) = element_to_remove {
                        let elem = persisted.scene.elements.remove(idx);
                        ctx.world_renderer.remove_instance(elem.instance);
                    }
                }

                if imgui::CollapsingHeader::new("Overrides")
                    .default_open(false)
                    .build(ui)
                {
                    macro_rules! do_flag {
                        ($flag:path, $name:literal) => {
                            let mut is_set: bool =
                                ctx.world_renderer.render_overrides.has_flag($flag);
                            ui.checkbox($name, &mut is_set);
                            ctx.world_renderer.render_overrides.set_flag($flag, is_set);
                        };
                    }

                    do_flag!(
                        RenderOverrideFlags::FORCE_FACE_NORMALS,
                        "Force face normals"
                    );
                    do_flag!(RenderOverrideFlags::NO_NORMAL_MAPS, "No normal maps");
                    do_flag!(
                        RenderOverrideFlags::FLIP_NORMAL_MAP_YZ,
                        "Flip normal map YZ"
                    );
                    do_flag!(RenderOverrideFlags::NO_METAL, "No metal");

                    imgui::Drag::<f32, &str>::new("Roughness scale")
                        .range(0.0, 4.0)
                        .speed(0.001)
                        .build(
                            ui,
                            &mut ctx.world_renderer.render_overrides.material_roughness_scale,
                        );
                }

                if imgui::CollapsingHeader::new("Sequence")
                    .default_open(false)
                    .build(ui)
                {
                    if ui.button("Add key") {
                        self.add_sequence_keyframe(persisted);
                    }

                    ui.same_line();
                    if self.is_sequence_playing() {
                        if ui.button("Stop") {
                            self.stop_sequence();
                        }
                    } else if ui.button("Play") {
                        self.play_sequence(persisted);
                    }

                    ui.same_line();
                    ui.set_next_item_width(60.0);
                    imgui::Drag::<f32, &str>::new("Speed")
                        .range(0.0, 4.0)
                        .speed(0.01)
                        .build(ui, &mut self.sequence_playback_speed);

                    if self.active_camera_key.is_some() {
                        ui.same_line();
                        if ui.button("Deselect key") {
                            self.active_camera_key = None;
                        }
                    }

                    enum Cmd {
                        JumpToKey(usize),
                        DeleteKey(usize),
                        ReplaceKey(usize),
                        None,
                    }
                    let mut cmd = Cmd::None;

                    persisted.sequence.each_key(|i, item| {
                        let active = Some(i) == self.active_camera_key;

                        let label = if active {
                            format!("-> {}:", i)
                        } else {
                            format!("{}:", i)
                        };

                        if ui.button(&label) {
                            cmd = Cmd::JumpToKey(i);
                        }

                        ui.same_line();
                        ui.set_next_item_width(60.0);
                        imgui::InputFloat::new(ui, format!(&"duration##{}", i), &mut item.duration)
                            .build();

                        ui.same_line();
                        ui.checkbox(
                            format!(&"Pos##{}", i),
                            &mut item.value.camera_position.is_some,
                        );

                        ui.same_line();
                        ui.checkbox(
                            format!(&"Dir##{}", i),
                            &mut item.value.camera_direction.is_some,
                        );

                        ui.same_line();
                        ui.checkbox(format!(&"Sun##{}", i), &mut item.value.towards_sun.is_some);

                        ui.same_line();
                        if ui.button(format!(&"Delete##{}", i)) {
                            cmd = Cmd::DeleteKey(i);
                        }

                        ui.same_line();
                        if ui.button(format!(&"Replace##{}:", i)) {
                            cmd = Cmd::ReplaceKey(i);
                        }
                    });

                    match cmd {
                        Cmd::JumpToKey(i) => self.jump_to_sequence_key(persisted, i),
                        Cmd::DeleteKey(i) => self.delete_camera_sequence_key(persisted, i),
                        Cmd::ReplaceKey(i) => self.replace_camera_sequence_key(persisted, i),
                        Cmd::None => {}
                    }
                }

                if imgui::CollapsingHeader::new("Debug")
                    .default_open(false)
                    .build(ui)
                {
                    if ui.radio_button_bool(
                        "Scene geometry",
                        ctx.world_renderer.debug_mode == RenderDebugMode::None,
                    ) {
                        ctx.world_renderer.debug_mode = RenderDebugMode::None;
                    }

                    /*if ui.radio_button_bool(
                        "World radiance cache"),
                        ctx.world_renderer.debug_mode == RenderDebugMode::WorldRadianceCache,
                    ) {
                        ctx.world_renderer.debug_mode = RenderDebugMode::WorldRadianceCache;
                    }*/

                    ui.combo(
                        "Shading",
                        &mut ctx.world_renderer.debug_shading_mode,
                        &[
                            "Default",
                            "No base color",
                            "Diffuse GI",
                            "Reflections",
                            "RTX OFF",
                            "Irradiance cache",
                        ],
                        |itm| Cow::Borrowed(itm),
                    );

                    imgui::Drag::<u32, &str>::new("Max FPS")
                        .range(1, MAX_FPS_LIMIT)
                        .build(ui, &mut self.max_fps);

                    ui.checkbox("Allow pass overlap", unsafe {
                        &mut kajiya::rg::RG_ALLOW_PASS_OVERLAP
                    });
                }

                if imgui::CollapsingHeader::new("GPU passes")
                    .default_open(true)
                    .build(ui)
                {
                    ui.text(format!("CPU frame time: {:.3}ms", ctx.dt_filtered * 1000.0));

                    if let Some(report) = gpu_profiler::profiler().last_report() {
                        let ordered_scopes = report.scopes.as_slice();
                        let gpu_time_ms: f64 =
                            ordered_scopes.iter().map(|scope| scope.duration.ms()).sum();

                        ui.text(format!("GPU frame time: {:.3}ms", gpu_time_ms));

                        for (scope_index, scope) in ordered_scopes.iter().enumerate() {
                            if scope.name == "debug" || scope.name.starts_with('_') {
                                continue;
                            }

                            let render_debug_hook = kajiya::rg::RenderDebugHook {
                                name: scope.name.clone(),
                                id: scope_index as u64,
                            };

                            let style = self.locked_rg_debug_hook.as_ref().and_then(|hook| {
                                if hook.render_debug_hook == render_debug_hook {
                                    Some(ui.push_style_color(
                                        imgui::StyleColor::Text,
                                        [1.0, 1.0, 0.1, 1.0],
                                    ))
                                } else {
                                    None
                                }
                            });

                            ui.text(format!("{}: {:.3}ms", scope.name, scope.duration.ms()));

                            if let Some(style) = style {
                                style.pop();
                            }

                            if ui.is_item_hovered() {
                                ctx.world_renderer.rg_debug_hook =
                                    Some(kajiya::rg::GraphDebugHook { render_debug_hook });

                                if ui.is_item_clicked() {
                                    if self.locked_rg_debug_hook == ctx.world_renderer.rg_debug_hook
                                    {
                                        self.locked_rg_debug_hook = None;
                                    } else {
                                        self.locked_rg_debug_hook =
                                            ctx.world_renderer.rg_debug_hook.clone();
                                    }
                                }
                            }
                        }
                    }
                }
            });
        }
    }
}
