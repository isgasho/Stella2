//! The backend for a *nix system.
//!
//! This backend is backed by the following software components:
//!
//!  - winit (usually backed by X11 or Wayland) for window management
//!  - Vulkan (WIP) or a software renderer for composition,
//!  - Cairo for 2D drawing (WIP)
//!  - FreeType/Pango/fontconfig for text rendering (WIP).
//!
use cggeom::Box2;
use cgmath::{Matrix3, Point2};
use std::marker::PhantomData;

use super::{
    iface,
    prelude::MtLazyStatic,
    winit::{HWndCore, WinitEnv, WinitWm, WinitWmCore},
};

// Define a global instance of `WinitEnv`.
//
// This is a part of boilerplate code of `super::winit` that exists because we
// delegate the window management to `super::winit`.
static WINIT_ENV: WinitEnv<Wm, WndContent> = WinitEnv::new();

pub type WndAttrs<'a> = iface::WndAttrs<'a, Wm, HLayer>;
pub type LayerAttrs = iface::LayerAttrs<Bitmap, HLayer>;
pub type CharStyleAttrs = iface::CharStyleAttrs<CharStyle>;

pub type HWnd = HWndCore;

mod comp;
pub use self::comp::{HLayer, WndContent};

/// Provides an access to the window system.
///
/// `Wm` is only accessible by the application's main thread. Therefore, the
/// ownership of `Wm` can be used as an evidence that the main thread has the
/// control.
#[derive(Debug, Clone, Copy)]
pub struct Wm {
    _no_send_sync: std::marker::PhantomData<*mut ()>,
}

mt_lazy_static! {
    static ref COMP: comp::Compositor => |wm| comp::Compositor::new(wm);
}

impl Wm {
    /// Get the global `WinitWmCore` instance.
    ///
    /// Use `WinitWmCore::wm` for the conversion in the other way around.
    fn winit_wm_core(self) -> &'static WinitWmCore<Wm, WndContent> {
        WINIT_ENV.wm_with_wm(self)
    }

    /// Get the global `Compositor` instance.
    fn comp(self) -> &'static comp::Compositor {
        COMP.get_with_wm(self)
    }
}

// `super::winit` uses this `impl` for the framework's operation
impl WinitWm for Wm {
    fn hwnd_core_to_hwnd(self, hwnd: &HWndCore) -> Self::HWnd {
        hwnd.clone()
    }

    fn init(self) {
        // Force the initialization of `COMP`. We should this now because if
        // we do it later, we might not be able to access winit's `EventLoop`,
        // which we need to initialize `Compositor`.
        //
        // Astoundingly un-Rusty... TODO: Perhaps make this more Rusty?
        // I think we could add a new type parameter to `WinitEnv` or a new
        // associate type to `WinitWm` to allow storing custom data in
        // `WinitWmCore`. Note that we can't store it in `Wm` because `Wm` is
        // just a marker type indicating the main thread. But, do not forget
        // to think about the practical benefits! (Do not blindly follow the
        // "best practices".)
        let _ = COMP.get_with_wm(self);
    }
}

impl iface::Wm for Wm {
    type HWnd = HWnd;
    type HLayer = HLayer;
    type Bitmap = Bitmap;

    unsafe fn global_unchecked() -> Wm {
        Wm {
            _no_send_sync: PhantomData,
        }
    }

    fn is_main_thread() -> bool {
        WINIT_ENV.is_main_thread()
    }

    fn invoke_on_main_thread(f: impl FnOnce(Wm) + Send + 'static) {
        WINIT_ENV.invoke_on_main_thread(move |winit_wm| f(winit_wm.wm()));
    }

    fn invoke(self, f: impl FnOnce(Self) + 'static) {
        self.winit_wm_core()
            .invoke(move |winit_wm| f(winit_wm.wm()));
    }

    fn enter_main_loop(self) -> ! {
        WINIT_ENV.wm_with_wm(self).enter_main_loop();
    }

    fn terminate(self) {
        WINIT_ENV.wm_with_wm(self).terminate();
    }

    fn new_wnd(self, attrs: WndAttrs<'_>) -> Self::HWnd {
        self.winit_wm_core().new_wnd(attrs, |winit_wnd, layer| {
            self.comp().new_wnd(winit_wnd, layer)
        })
    }

    fn set_wnd_attr(self, hwnd: &Self::HWnd, attrs: WndAttrs<'_>) {
        self.winit_wm_core().set_wnd_attr(hwnd, attrs)
    }

    fn remove_wnd(self, hwnd: &Self::HWnd) {
        self.winit_wm_core().remove_wnd(hwnd)
    }

    fn update_wnd(self, hwnd: &Self::HWnd) {
        self.winit_wm_core().update_wnd(hwnd)
    }

    fn get_wnd_size(self, hwnd: &Self::HWnd) -> [u32; 2] {
        self.winit_wm_core().get_wnd_size(hwnd)
    }

    fn get_wnd_dpi_scale(self, hwnd: &Self::HWnd) -> f32 {
        self.winit_wm_core().get_wnd_dpi_scale(hwnd)
    }

    fn new_layer(self, attrs: LayerAttrs) -> Self::HLayer {
        self.comp().new_layer(attrs)
    }
    fn set_layer_attr(self, layer: &Self::HLayer, attrs: LayerAttrs) {
        self.comp().set_layer_attr(layer, attrs)
    }
    fn remove_layer(self, layer: &Self::HLayer) {
        self.comp().remove_layer(layer)
    }
}

// The following types are all TODO
#[derive(Debug, Clone)]
pub struct Bitmap;

impl iface::Bitmap for Bitmap {
    fn size(&self) -> [u32; 2] {
        unimplemented!()
    }
}

#[derive(Debug)]
pub struct BitmapBuilder;

impl iface::BitmapBuilderNew for BitmapBuilder {
    fn new(size: [u32; 2]) -> Self {
        unimplemented!()
    }
}

impl iface::BitmapBuilder for BitmapBuilder {
    type Bitmap = Bitmap;

    fn into_bitmap(self) -> Self::Bitmap {
        unimplemented!()
    }
}

impl iface::Canvas for BitmapBuilder {
    fn save(&mut self) {
        unimplemented!()
    }
    fn restore(&mut self) {
        unimplemented!()
    }
    fn begin_path(&mut self) {
        unimplemented!()
    }
    fn close_path(&mut self) {
        unimplemented!()
    }
    fn move_to(&mut self, p: Point2<f32>) {
        unimplemented!()
    }
    fn line_to(&mut self, p: Point2<f32>) {
        unimplemented!()
    }
    fn cubic_bezier_to(&mut self, cp1: Point2<f32>, cp2: Point2<f32>, p: Point2<f32>) {
        unimplemented!()
    }
    fn quad_bezier_to(&mut self, cp: Point2<f32>, p: Point2<f32>) {
        unimplemented!()
    }
    fn fill(&mut self) {
        unimplemented!()
    }
    fn stroke(&mut self) {
        unimplemented!()
    }
    fn clip(&mut self) {
        unimplemented!()
    }
    fn set_fill_rgb(&mut self, rgb: iface::RGBAF32) {
        unimplemented!()
    }
    fn set_stroke_rgb(&mut self, rgb: iface::RGBAF32) {
        unimplemented!()
    }
    fn set_line_cap(&mut self, cap: iface::LineCap) {
        unimplemented!()
    }
    fn set_line_join(&mut self, join: iface::LineJoin) {
        unimplemented!()
    }
    fn set_line_dash(&mut self, phase: f32, lengths: &[f32]) {
        unimplemented!()
    }
    fn set_line_width(&mut self, width: f32) {
        unimplemented!()
    }
    fn set_line_miter_limit(&mut self, miter_limit: f32) {
        unimplemented!()
    }
    fn mult_transform(&mut self, m: Matrix3<f32>) {
        unimplemented!()
    }
}

impl iface::CanvasText<TextLayout> for BitmapBuilder {
    fn draw_text(&mut self, layout: &TextLayout, origin: Point2<f32>, color: iface::RGBAF32) {
        unimplemented!()
    }
}

#[derive(Debug, Clone)]
pub struct CharStyle;

impl iface::CharStyle for CharStyle {
    fn new(attrs: CharStyleAttrs) -> Self {
        unimplemented!()
    }

    fn size(&self) -> f32 {
        unimplemented!()
    }
}

#[derive(Debug)]
pub struct TextLayout;

impl iface::TextLayout for TextLayout {
    type CharStyle = CharStyle;

    fn from_text(text: &str, style: &Self::CharStyle, width: Option<f32>) -> Self {
        unimplemented!()
    }

    fn visual_bounds(&self) -> Box2<f32> {
        unimplemented!()
    }

    fn layout_bounds(&self) -> Box2<f32> {
        unimplemented!()
    }
}