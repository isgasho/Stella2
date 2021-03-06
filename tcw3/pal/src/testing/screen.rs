//! Compositor for the testing backend.
use cggeom::{box2, prelude::*, Box2};
use cgmath::{Point2, Vector2};
use log::warn;
use std::{cell::RefCell, fmt, rc::Rc};

use super::super::{iface, swrast};
use super::{
    bitmap::Bitmap,
    uniqpool::{PoolPtr, UniqPool},
    wmapi, AccelTable, Wm,
};

pub type WndAttrs<'a> = iface::WndAttrs<'a, Wm, HLayer>;
pub type LayerAttrs = iface::LayerAttrs<Bitmap, HLayer>;

pub(super) struct Screen {
    state: RefCell<State>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct HWnd {
    /// A pointer into `State::wnds`.
    ptr: PoolPtr,
}

impl fmt::Debug for HWnd {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("HWnd").field(&self.ptr).finish()
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct HLayer {
    sr_layer: swrast::HLayer<Bitmap>,
}

impl fmt::Debug for HLayer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.sr_layer)
    }
}

struct State {
    binner: swrast::Binner<Bitmap>,
    sr_scrn: swrast::Screen<Bitmap>,
    wnds: UniqPool<Wnd>,
}

pub struct Wnd {
    sr_wnd: swrast::HWnd<Bitmap>,

    dpi_scale: f32,
    focused: bool,
    attrs: wmapi::WndAttrs,
    listener: Rc<dyn iface::WndListener<Wm>>,

    dirty_rect: Option<Box2<usize>>,
    img_size: [usize; 2],
    img_data: Vec<u8>,
    img_dpi_scale: f32,
}

impl Screen {
    pub(super) fn new() -> Self {
        let state = State {
            binner: swrast::Binner::new(),
            sr_scrn: swrast::Screen::new(),
            wnds: UniqPool::new(),
        };

        Self {
            state: RefCell::new(state),
        }
    }

    pub(super) fn reset(&self) {
        let mut state = self.state.borrow_mut();

        if state.wnds.iter().next().is_some() {
            warn!("Deleting {} stray window(s)", state.wnds.iter().count());
        }

        state.sr_scrn = swrast::Screen::new();
        state.wnds = UniqPool::new();
    }

    pub(super) fn new_wnd(&self, attrs: WndAttrs<'_>) -> HWnd {
        let mut state = self.state.borrow_mut();

        let layer = attrs.layer.unwrap_or(None);

        let wnd = Wnd {
            sr_wnd: state.sr_scrn.new_wnd(),
            dpi_scale: 1.0, // TODO
            focused: false,
            dirty_rect: None,
            attrs: wmapi::WndAttrs {
                size: attrs.size.unwrap_or([100, 100]),
                min_size: attrs.min_size.unwrap_or([0; 2]),
                max_size: attrs.max_size.unwrap_or([u32::max_value(); 2]),
                flags: attrs.flags.unwrap_or(iface::WndFlags::default()),
                caption: attrs.caption.unwrap_or("Default title".into()).into_owned(),
                visible: attrs.visible.unwrap_or(false),
                cursor_shape: attrs.cursor_shape.unwrap_or_default(),
            },
            listener: Rc::from(attrs.listener.unwrap_or_else(|| Box::new(()))),
            img_size: [0, 0],
            img_data: Vec::new(),
            img_dpi_scale: 1.0,
        };

        state
            .sr_scrn
            .set_wnd_layer(&wnd.sr_wnd, layer.map(|hl| hl.sr_layer));

        let ptr = state.wnds.allocate(wnd);
        HWnd { ptr }
    }

    pub(super) fn set_wnd_attr(&self, hwnd: &HWnd, attrs: WndAttrs<'_>) {
        let mut state = self.state.borrow_mut();
        let state = &mut *state; // enable split borrow

        let wnd = &mut state.wnds[hwnd.ptr];

        macro_rules! apply {
            ($name:ident) => {
                if let Some(value) = attrs.$name {
                    wnd.attrs.$name = value.into();
                }
            };
        }
        apply!(size);
        apply!(min_size);
        apply!(max_size);
        apply!(flags);
        apply!(caption);
        apply!(visible);
        apply!(cursor_shape);

        if let Some(layer) = attrs.layer {
            state
                .sr_scrn
                .set_wnd_layer(&wnd.sr_wnd, layer.map(|hl| hl.sr_layer));
        }

        if let Some(value) = attrs.listener {
            wnd.listener = Rc::from(value);
        }
    }
    pub(super) fn remove_wnd(&self, hwnd: &HWnd) {
        let mut state = self.state.borrow_mut();
        let state = &mut *state; // enable split borrow

        let wnd = state.wnds.deallocate(hwnd.ptr).expect("invalid hwnd");

        state.sr_scrn.remove_wnd(&wnd.sr_wnd);
    }
    pub(super) fn update_wnd(&self, hwnd: &HWnd) {
        let mut state = self.state.borrow_mut();
        let state = &mut *state; // enable split borrow
        let wnd: &mut Wnd = &mut state.wnds[hwnd.ptr];

        // Apply deferred changes and compute the dirty region
        if let Some(new_dirty) = state.sr_scrn.update_wnd(&wnd.sr_wnd) {
            if let Some(x) = &mut wnd.dirty_rect {
                x.union_assign(&new_dirty);
            } else {
                wnd.dirty_rect = Some(new_dirty);
            }
        }
    }
    pub(super) fn get_wnd_size(&self, hwnd: &HWnd) -> [u32; 2] {
        let state = self.state.borrow();
        state.wnds[hwnd.ptr].attrs.size
    }
    pub(super) fn get_wnd_dpi_scale(&self, hwnd: &HWnd) -> f32 {
        let state = self.state.borrow();
        state.wnds[hwnd.ptr].dpi_scale
    }
    pub(super) fn is_wnd_focused(&self, hwnd: &HWnd) -> bool {
        let state = self.state.borrow();
        state.wnds[hwnd.ptr].focused
    }

    pub(super) fn new_layer(&self, attrs: LayerAttrs) -> HLayer {
        let mut state = self.state.borrow_mut();

        HLayer {
            sr_layer: state
                .sr_scrn
                .new_layer(layer_attrs_to_sr_layer_attrs(attrs)),
        }
    }
    pub(super) fn set_layer_attr(&self, layer: &HLayer, attrs: LayerAttrs) {
        let mut state = self.state.borrow_mut();

        state
            .sr_scrn
            .set_layer_attr(&layer.sr_layer, layer_attrs_to_sr_layer_attrs(attrs));
    }
    pub(super) fn remove_layer(&self, layer: &HLayer) {
        let mut state = self.state.borrow_mut();

        state.sr_scrn.remove_layer(&layer.sr_layer);
    }

    /// Implements `TestingWm::hwnds`.
    pub(super) fn hwnds(&self) -> Vec<HWnd> {
        let state = self.state.borrow();

        state.wnds.ptr_iter().map(|(ptr, _)| HWnd { ptr }).collect()
    }

    /// Implements `TestingWm::wnd_attrs`.
    pub(super) fn wnd_attrs(&self, hwnd: &HWnd) -> Option<wmapi::WndAttrs> {
        let state = self.state.borrow();

        state.wnds.get(hwnd.ptr).map(|wnd| wnd.attrs.clone())
    }

    /// Get a `WndListener`.
    fn wnd_listener(&self, hwnd: &HWnd) -> Result<Rc<dyn iface::WndListener<Wm>>, BadHWndError> {
        let state = self.state.borrow();
        state
            .wnds
            .get(hwnd.ptr)
            .ok_or(BadHWndError)
            .map(|wnd| Rc::clone(&wnd.listener))
    }

    /// Implements `TestingWm::raise_close_requested`.
    pub(super) fn raise_close_requested(&self, wm: Wm, hwnd: &HWnd) {
        let listener = self.wnd_listener(hwnd).unwrap();

        listener.close_requested(wm, &hwnd.into());
    }

    pub(super) fn raise_update_ready(&self, wm: Wm, hwnd: &HWnd) {
        let listener = self.wnd_listener(hwnd).unwrap();

        listener.update_ready(wm, &hwnd.into());
    }

    /// Implements `TestingWm::set_wnd_dpi_scale`.
    pub(super) fn set_wnd_dpi_scale(&self, wm: Wm, hwnd: &HWnd, dpi_scale: f32) {
        assert!(dpi_scale > 0.0);
        assert!(dpi_scale.is_finite());

        let mut state = self.state.borrow_mut();
        state.wnds[hwnd.ptr].dpi_scale = dpi_scale;
        drop(state);

        let listener = self.wnd_listener(hwnd).unwrap();
        listener.dpi_scale_changed(wm, &hwnd.into());
    }

    /// Implements `TestingWm::set_wnd_size`.
    pub(super) fn set_wnd_size(&self, wm: Wm, hwnd: &HWnd, size: [u32; 2]) {
        let mut state = self.state.borrow_mut();
        state.wnds[hwnd.ptr].attrs.size = size;
        drop(state);

        let listener = self.wnd_listener(hwnd).unwrap();
        listener.resize(wm, &hwnd.into());
    }

    /// Implements `TestingWm::set_wnd_focused`.
    pub(super) fn set_wnd_focused(&self, wm: Wm, hwnd: &HWnd, focused: bool) {
        let mut state = self.state.borrow_mut();
        state.wnds[hwnd.ptr].focused = focused;
        drop(state);

        let listener = self.wnd_listener(hwnd).unwrap();
        listener.focus(wm, &hwnd.into());
    }

    /// Implements `TestingWm::read_wnd_snapshot`.
    pub(super) fn read_wnd_snapshot(&self, hwnd: &HWnd, out: &mut wmapi::WndSnapshot) {
        let mut state = self.state.borrow_mut();
        let state = &mut *state; // enable split borrow
        let wnd: &mut Wnd = &mut state.wnds[hwnd.ptr];

        // Calculate the surface size
        let [size_w, size_h] = wnd.attrs.size;
        let dpi_scale = wnd.dpi_scale;
        let surf_size = [
            (size_w as f32 * dpi_scale) as usize,
            (size_h as f32 * dpi_scale) as usize,
        ];

        if surf_size[0] == 0 || surf_size[1] == 0 {
            // Suspend update if one of the surface dimensions is zero
            out.size = [0, 0];
            out.data.clear();
            out.stride = 0;
            return;
        }

        let img_stride = 4usize.checked_mul(surf_size[0]).unwrap();
        let img_stride = img_stride.checked_add(63).unwrap() & !63;
        let num_bytes = img_stride.checked_mul(surf_size[1]).unwrap();

        if (surf_size, dpi_scale) != (wnd.img_size, wnd.img_dpi_scale) {
            wnd.dirty_rect = Some(box2! { min: [0, 0].into(), max: surf_size.into() });
            wnd.img_size = surf_size;
            wnd.img_data.resize(num_bytes, 0);

            state.sr_scrn.set_wnd_size(&wnd.sr_wnd, surf_size);
            state.sr_scrn.set_wnd_dpi_scale(&wnd.sr_wnd, wnd.dpi_scale);
        }

        // Update the backing store
        if let Some(dirty_rect) = wnd.dirty_rect.take() {
            state.sr_scrn.render_wnd(
                &wnd.sr_wnd,
                &mut wnd.img_data,
                img_stride,
                dirty_rect,
                &mut state.binner,
            );
        }

        // Copy that to the given buffer, `out`
        out.size = surf_size;
        out.stride = img_stride;
        out.data.clear();
        out.data.extend(&wnd.img_data[..]);
    }

    /// Implements `TestingWm::raise_mouse_motion`.
    pub(super) fn raise_mouse_motion(&self, wm: Wm, hwnd: &HWnd, loc: Point2<f32>) {
        let listener = self.wnd_listener(hwnd).unwrap();

        listener.mouse_motion(wm, &hwnd.into(), loc);
    }

    /// Implements `TestingWm::raise_mouse_leave`.
    pub(super) fn raise_mouse_leave(&self, wm: Wm, hwnd: &HWnd) {
        let listener = self.wnd_listener(hwnd).unwrap();

        listener.mouse_leave(wm, &hwnd.into());
    }

    /// Implements `TestingWm::raise_mouse_drag`.
    pub(super) fn raise_mouse_drag(
        &self,
        wm: Wm,
        hwnd: &HWnd,
        loc: Point2<f32>,
        button: u8,
    ) -> Box<dyn wmapi::MouseDrag> {
        let listener = self.wnd_listener(hwnd).unwrap();

        let inner = listener.mouse_drag(wm, &hwnd.into(), loc, button);

        Box::new(MouseDrag {
            wm,
            hwnd: hwnd.into(),
            inner,
        })
    }

    /// Implements `TestingWm::raise_scroll_motion`.
    pub(super) fn raise_scroll_motion(
        &self,
        wm: Wm,
        hwnd: &HWnd,
        loc: Point2<f32>,
        delta: &iface::ScrollDelta,
    ) {
        let listener = self.wnd_listener(hwnd).unwrap();

        listener.scroll_motion(wm, &hwnd.into(), loc, delta);
    }

    /// Implements `TestingWm::raise_scroll_gesture`.
    pub(super) fn raise_scroll_gesture(
        &self,
        wm: Wm,
        hwnd: &HWnd,
        loc: Point2<f32>,
    ) -> Box<dyn wmapi::ScrollGesture> {
        let listener = self.wnd_listener(hwnd).unwrap();

        let inner = listener.scroll_gesture(wm, &hwnd.into(), loc);

        Box::new(ScrollGesture {
            wm,
            hwnd: hwnd.into(),
            inner,
        })
    }

    /// Implements `TestingWm::translate_action`.
    pub(super) fn translate_action(
        &self,
        wm: Wm,
        hwnd: &HWnd,
        source: &str,
        pattern: &str,
    ) -> Option<iface::ActionId> {
        let listener = self.wnd_listener(hwnd).unwrap();

        struct EnumAccel<F: FnMut(&AccelTable)>(F);
        impl<F: FnMut(&AccelTable)> iface::InterpretEventCtx<AccelTable> for EnumAccel<F> {
            fn use_accel(&mut self, accel: &AccelTable) {
                (self.0)(accel);
            }
        }

        let mut action = None;
        listener.interpret_event(
            wm,
            &hwnd.into(),
            &mut EnumAccel(|accel_table| {
                if action.is_none() {
                    action = accel_table
                        .testing
                        .iter()
                        .find(|binding| (binding.source, binding.pattern) == (source, pattern))
                        .map(|binding| binding.action);
                }
            }),
        );

        action
    }

    /// Implements `TestingWm::raise_validate_action`.
    pub(super) fn raise_validate_action(
        &self,
        wm: Wm,
        hwnd: &HWnd,
        action: iface::ActionId,
    ) -> iface::ActionStatus {
        let listener = self.wnd_listener(hwnd).unwrap();
        listener.validate_action(wm, &hwnd.into(), action)
    }

    /// Implements `TestingWm::raise_perform_action`.
    pub(super) fn raise_perform_action(&self, wm: Wm, hwnd: &HWnd, action: iface::ActionId) {
        let listener = self.wnd_listener(hwnd).unwrap();
        listener.perform_action(wm, &hwnd.into(), action);
    }

    /// Implements `TestingWm::raise_key_down`.
    pub(super) fn raise_key_down(&self, wm: Wm, hwnd: &HWnd, source: &str, pattern: &str) -> bool {
        let listener = self.wnd_listener(hwnd).unwrap();
        listener.key_down(wm, &hwnd.into(), &SimulatedKeyEvent { source, pattern })
    }

    /// Implements `TestingWm::raise_key_up`.
    pub(super) fn raise_key_up(&self, wm: Wm, hwnd: &HWnd, source: &str, pattern: &str) -> bool {
        let listener = self.wnd_listener(hwnd).unwrap();
        listener.key_up(wm, &hwnd.into(), &SimulatedKeyEvent { source, pattern })
    }
}

#[derive(Debug)]
struct BadHWndError;

/// Convert the `LayerAttrs` of `Wm` to the `LayerAttrs` of `swrast`.
/// Copied straight from `unix/comp.rs`.
fn layer_attrs_to_sr_layer_attrs(
    attrs: LayerAttrs,
) -> iface::LayerAttrs<Bitmap, swrast::HLayer<Bitmap>> {
    iface::LayerAttrs {
        transform: attrs.transform,
        contents: attrs.contents,
        bounds: attrs.bounds,
        contents_center: attrs.contents_center,
        contents_scale: attrs.contents_scale,
        bg_color: attrs.bg_color,
        sublayers: attrs.sublayers.map(|sublayers| {
            sublayers
                .into_iter()
                .map(|hlayer| hlayer.sr_layer)
                .collect()
        }),
        opacity: attrs.opacity,
        flags: attrs.flags,
    }
}

struct MouseDrag {
    wm: Wm,
    hwnd: super::HWnd,
    inner: Box<dyn iface::MouseDragListener<Wm>>,
}

impl wmapi::MouseDrag for MouseDrag {
    fn mouse_motion(&self, loc: Point2<f32>) {
        self.inner.mouse_motion(self.wm, &self.hwnd, loc)
    }
    fn mouse_down(&self, loc: Point2<f32>, button: u8) {
        self.inner.mouse_down(self.wm, &self.hwnd, loc, button)
    }
    fn mouse_up(&self, loc: Point2<f32>, button: u8) {
        self.inner.mouse_up(self.wm, &self.hwnd, loc, button)
    }
    fn cancel(&self) {
        self.inner.cancel(self.wm, &self.hwnd)
    }
}

struct ScrollGesture {
    wm: Wm,
    hwnd: super::HWnd,
    inner: Box<dyn iface::ScrollListener<Wm>>,
}

impl wmapi::ScrollGesture for ScrollGesture {
    fn motion(&self, delta: &iface::ScrollDelta, velocity: Vector2<f32>) {
        self.inner.motion(self.wm, &self.hwnd, delta, velocity)
    }
    fn start_momentum_phase(&self) {
        self.inner.start_momentum_phase(self.wm, &self.hwnd)
    }
    fn end(&self) {
        self.inner.end(self.wm, &self.hwnd)
    }
    fn cancel(&self) {
        self.inner.cancel(self.wm, &self.hwnd)
    }
}

struct SimulatedKeyEvent<'a> {
    source: &'a str,
    pattern: &'a str,
}

impl iface::KeyEvent<AccelTable> for SimulatedKeyEvent<'_> {
    fn translate_accel(&self, accel_table: &AccelTable) -> Option<iface::ActionId> {
        accel_table
            .testing
            .iter()
            .find(|binding| (binding.source, binding.pattern) == (self.source, self.pattern))
            .map(|binding| binding.action)
    }
}
