#![allow(dead_code)]

use glam::Vec2;
use std::collections::HashMap;
pub use winit::event::ElementState;
use winit::{
    event::{Event, WindowEvent},
    keyboard::PhysicalKey,
};

pub type PhysicalPosition = winit::dpi::PhysicalPosition<f64>;

#[derive(Clone)]
pub struct KeyState {
    pub ticks: u32,
}

#[derive(Default, Clone)]
pub struct KeyboardState {
    keys_down: HashMap<PhysicalKey, KeyState>,
}

impl KeyboardState {
    pub fn is_down(&self, key: PhysicalKey) -> bool {
        self.get_down(key).is_some()
    }

    pub fn was_just_pressed(&self, key: PhysicalKey) -> bool {
        self.get_down(key).map(|s| s.ticks == 1).unwrap_or_default()
    }

    pub fn get_down(&self, key: PhysicalKey) -> Option<&KeyState> {
        self.keys_down.get(&key)
    }

    pub fn update(&mut self, events: &[Event<()>]) {
        for event in events {
            if let Event::WindowEvent {
                event: WindowEvent::KeyboardInput { event, .. },
                ..
            } = event
            {
                if event.state == ElementState::Pressed {
                    self.keys_down
                        .entry(event.physical_key)
                        .or_insert(KeyState { ticks: 0 });
                } else {
                    self.keys_down.remove(&event.physical_key);
                }
            }
        }

        for ks in self.keys_down.values_mut() {
            ks.ticks += 1;
        }
    }
}

#[derive(Clone, Copy)]
pub struct MouseState {
    pub physical_position: PhysicalPosition,
    pub delta: Vec2,
    pub buttons_held: u32,
    pub buttons_pressed: u32,
    pub buttons_released: u32,
}

impl Default for MouseState {
    fn default() -> Self {
        Self {
            physical_position: PhysicalPosition { x: 0.0, y: 0.0 },
            delta: Vec2::ZERO,
            buttons_held: 0,
            buttons_pressed: 0,
            buttons_released: 0,
        }
    }
}

impl MouseState {
    pub fn update(&mut self, events: &[Event<()>]) {
        self.buttons_pressed = 0;
        self.buttons_released = 0;
        self.delta = Vec2::ZERO;

        for event in events {
            match event {
                Event::WindowEvent { event, .. } => match event {
                    WindowEvent::CursorMoved { position, .. } => {
                        self.physical_position = *position;
                    }
                    WindowEvent::MouseInput { state, button, .. } => {
                        let button_id = match button {
                            winit::event::MouseButton::Left => 0,
                            winit::event::MouseButton::Middle => 1,
                            winit::event::MouseButton::Right => 2,
                            _ => 0,
                        };

                        if let ElementState::Pressed = state {
                            self.buttons_held |= 1 << button_id;
                            self.buttons_pressed |= 1 << button_id;
                        } else {
                            self.buttons_held &= !(1 << button_id);
                            self.buttons_released |= 1 << button_id;
                        }
                    }
                    _ => (),
                },
                Event::DeviceEvent {
                    device_id: _,
                    event: winit::event::DeviceEvent::MouseMotion { delta },
                } => {
                    self.delta.x += delta.0 as f32;
                    self.delta.y += delta.1 as f32;
                }
                _ => (),
            }
        }
    }
}

pub type InputAxis = &'static str;

pub struct KeyMap {
    axis: InputAxis,
    multiplier: f32,
    activation_time: f32,
}

impl KeyMap {
    pub fn new(axis: InputAxis, multiplier: f32) -> Self {
        Self {
            axis,
            multiplier,
            activation_time: 0.15,
        }
    }

    pub fn activation_time(mut self, value: f32) -> Self {
        self.activation_time = value;
        self
    }
}

struct KeyMapState {
    map: KeyMap,
    activation: f32,
}

pub struct KeyboardMap {
    bindings: Vec<(PhysicalKey, KeyMapState)>,
}

impl Default for KeyboardMap {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyboardMap {
    pub fn new() -> Self {
        Self {
            bindings: Default::default(),
        }
    }

    pub fn bind(mut self, key: PhysicalKey, map: KeyMap) -> Self {
        self.bindings.push((
            key,
            KeyMapState {
                map,
                activation: 0.0,
            },
        ));
        self
    }

    pub fn map(&mut self, keyboard: &KeyboardState, dt: f32) -> HashMap<InputAxis, f32> {
        let mut result: HashMap<InputAxis, f32> = HashMap::new();

        for (vk, s) in &mut self.bindings {
            #[allow(clippy::collapsible_else_if)]
            if s.map.activation_time > 1e-10 {
                let change = if keyboard.is_down(*vk) { dt } else { -dt };
                s.activation = (s.activation + change / s.map.activation_time).clamp(0.0, 1.0);
            } else {
                if keyboard.is_down(*vk) {
                    s.activation = 1.0;
                } else {
                    s.activation = 0.0;
                }
            }

            *result.entry(s.map.axis).or_default() += s.activation.powi(2) * s.map.multiplier;
        }

        for value in result.values_mut() {
            *value = value.clamp(-1.0, 1.0);
        }

        result
    }
}
