//! Maps `Bitmap` to `CompositionDrawingSurface`.
use atom2::SetOnce;
use direct2d::{math::SizeU, RenderTarget};
use std::{
    mem::MaybeUninit,
    ptr::{null, null_mut},
};
use winapi::{
    shared::{dxgi::IDXGIDevice, ntdef::HRESULT, windef::POINT},
    um::{
        d2d1_1::{D2D1CreateDevice, ID2D1Device, ID2D1DeviceContext},
        d3d11, d3dcommon,
        dcommon::D2D_SIZE_U,
        handleapi::CloseHandle,
        synchapi::CreateEventW,
        threadpoolapiset,
        unknwnbase::IUnknown,
        winnt::{HANDLE, PTP_CALLBACK_INSTANCE, PTP_WAIT, PVOID, TP_WAIT_RESULT},
    },
    Interface,
};
use winrt::{
    windows::graphics::directx::{DirectXAlphaMode, DirectXPixelFormat},
    windows::graphics::SizeInt32,
    windows::ui::composition::{Compositor, ICompositionGraphicsDevice2, ICompositionSurface},
    ComPtr,
};

use super::{
    bitmap::Bitmap,
    utils::{
        assert_hresult_ok, assert_win32_nonnull, assert_win32_ok, panic_hresult, ComPtr as MyComPtr,
    },
    winapiext::{
        ICompositionDrawingSurfaceInterop, ICompositionGraphicsDeviceInterop, ICompositorInterop,
        ID3D11Device4,
    },
    Wm,
};
use crate::MtLock;

/// Maps `Bitmap` to `CompositionDrawingSurface`.
pub struct SurfaceMap {
    comp_device2: ComPtr<ICompositionGraphicsDevice2>,
}

impl SurfaceMap {
    pub fn new(comp: &Compositor) -> Self {
        let (comp_idevice, d3d_device) = repeat_until_devlost_is_resolved_nodelay(|| {
            // Create the initial device
            let (d3d_device, d2d_device) = new_render_device()?;

            // Create `CompositionGraphicsDevice`
            let comp = unsafe { MyComPtr::from_ptr_unchecked(comp as *const _ as *mut IUnknown) };
            unsafe { comp.AddRef() };

            let comp_interop: MyComPtr<ICompositorInterop> = comp.query_interface().unwrap();

            let comp_idevice = unsafe {
                let mut out = MaybeUninit::uninit();
                assert_hresult_ok_or_devlost(
                    comp_interop.CreateGraphicsDevice(d2d_device.as_ptr() as _, out.as_mut_ptr()),
                )?;
                ComPtr::wrap(out.assume_init())
            };

            Ok((comp_idevice, d3d_device))
        });

        let comp_device2: ComPtr<ICompositionGraphicsDevice2> = comp_idevice
            .query_interface()
            .expect("Could not obtain ICompositionGraphicsDevice2");

        let comp_device_interop: MyComPtr<ICompositionGraphicsDeviceInterop> =
            MyComPtr::iunknown_from_winrt_comptr(comp_idevice)
                .query_interface()
                .unwrap();

        // Listen for device lost events and recreate objects appropriately
        listen_for_device_lost_events(comp_device_interop, d3d_device);

        Self { comp_device2 }
    }
}

/// Listen for a device lost event generated by `d3d_device`. If the event is
/// received, recreate a device using `new_render_device`, register it to
/// `comp_device_interop`, and continue listening using the new device.
///
/// There is no way to stop this.
fn listen_for_device_lost_events(
    comp_device_interop: MyComPtr<ICompositionGraphicsDeviceInterop>,
    d3d_device: MyComPtr<ID3D11Device4>,
) {
    use std::{
        cell::UnsafeCell,
        sync::atomic::{AtomicU32, Ordering},
    };

    struct ListenCtx {
        comp_device_interop: MyComPtr<ICompositionGraphicsDeviceInterop>,
        d3d_device: MyComPtr<ID3D11Device4>,
        cookie: AtomicU32,
    }

    unsafe impl Send for ListenCtx {} // FIXME: dubious

    fn listen(ctx: Box<UnsafeCell<ListenCtx>>) {
        let ctx_ref = unsafe { &*ctx.get() };

        // `ctx` is moved into the closure. The closure will be called when
        // `evt` is set in response to a "device removed" event.
        let evt = register_event_cb(move |evt| {
            let ctx_ref = unsafe { &*ctx.get() };

            // Wait if `cookie` isn't set yet
            while ctx_ref.cookie.load(Ordering::Acquire) == 0 {
                std::thread::yield_now();
            }

            // Now we have an exclusive access to `ctx`
            let ctx_ref = unsafe { &mut *ctx.get() };

            let cookie = *ctx_ref.cookie.get_mut();

            log::info!(
                "Received device removal event for {:?} (cookie = {:?})",
                ctx_ref.d3d_device.as_ptr(),
                cookie
            );

            // Unregister the event from the device using the `cookie`
            // returned by `RegisterDeviceRemovedEvent`
            unsafe { ctx_ref.d3d_device.UnregisterDeviceRemoved(cookie) };

            assert_win32_ok(unsafe { CloseHandle(evt) });

            // Create a new set of devices
            let (d3d_device, d2d_device) =
                repeat_until_devlost_is_resolved_nodelay(new_render_device);
            assert_hresult_ok(unsafe {
                ctx_ref
                    .comp_device_interop
                    .SetRenderingDevice(d2d_device.as_ptr() as _)
            });

            // Continue listening
            ctx_ref.d3d_device = d3d_device;
            listen(ctx);
        });

        // We have a shared access to `ctx` until `cookie` is set
        let mut cookie = MaybeUninit::uninit();
        assert_hresult_ok(unsafe {
            ctx_ref
                .d3d_device
                .RegisterDeviceRemovedEvent(evt, cookie.as_mut_ptr())
        });
        let cookie = unsafe { cookie.assume_init() };
        assert_ne!(cookie, 0);

        log::debug!(
            "Watching for device removal event on {:?} (cookie = {:?})",
            ctx_ref.d3d_device.as_ptr(),
            cookie
        );

        ctx_ref.cookie.store(cookie, Ordering::Release);
    }

    listen(Box::new(UnsafeCell::new(ListenCtx {
        comp_device_interop,
        d3d_device,
        cookie: AtomicU32::new(0),
    })));
}

fn new_render_device() -> Result<(MyComPtr<ID3D11Device4>, MyComPtr<ID2D1Device>), DeviceLost> {
    let feature_levels = &[
        d3dcommon::D3D_FEATURE_LEVEL_11_1,
        d3dcommon::D3D_FEATURE_LEVEL_11_0,
        d3dcommon::D3D_FEATURE_LEVEL_10_1,
        d3dcommon::D3D_FEATURE_LEVEL_10_0,
        d3dcommon::D3D_FEATURE_LEVEL_9_3,
        d3dcommon::D3D_FEATURE_LEVEL_9_2,
        d3dcommon::D3D_FEATURE_LEVEL_9_1,
    ];

    // Create a Direct3D 11 device. This will succeed whether a supported GPU
    // is installed or not (by falling back to the "basic display driver" if
    // necessary).
    let d3d11_device = unsafe {
        let mut out = MaybeUninit::uninit();
        assert_hresult_ok_or_devlost(d3d11::D3D11CreateDevice(
            null_mut(), // default adapter
            d3dcommon::D3D_DRIVER_TYPE_HARDWARE,
            null_mut(), // not asking for a SW driver, so not passing a module to one
            d3d11::D3D11_CREATE_DEVICE_BGRA_SUPPORT, // needed for Direct2D
            feature_levels.as_ptr(),
            feature_levels.len() as _,
            d3d11::D3D11_SDK_VERSION,
            out.as_mut_ptr(),
            null_mut(), // not interested in which feature level is chosen
            null_mut(), // not interested in `ID3D11DeviceContext`
        ))?;
        MyComPtr::from_ptr_unchecked(out.assume_init())
    };

    // Get `ID3D11Device4`
    let d3d11_device4: MyComPtr<ID3D11Device4> = d3d11_device
        .query_interface()
        .expect("Could not obtain ID3D11Device4");

    // Create Direct2D device
    let dxgi_device: MyComPtr<IDXGIDevice> = d3d11_device.query_interface().unwrap();
    let d2d_device = unsafe {
        let mut out = MaybeUninit::uninit();
        assert_hresult_ok_or_devlost(D2D1CreateDevice(&*dxgi_device, null(), out.as_mut_ptr()))?;
        MyComPtr::from_ptr_unchecked(out.assume_init())
    };

    Ok((d3d11_device4, d2d_device))
}

/// Create a Win32 event and register a function to be called when the event
/// is set. The event handle is *not* automatically closed.
///
/// There is no way to cancel the operation apart from setting the event.
fn register_event_cb<T: FnOnce(HANDLE) + Send>(f: T) -> HANDLE {
    struct Ctx<T> {
        f: T,
        evt: HANDLE,
    }

    unsafe extern "system" fn handler<T: FnOnce(HANDLE)>(
        _nstance: PTP_CALLBACK_INSTANCE,
        ctx: PVOID,
        wait: PTP_WAIT,
        _wait_result: TP_WAIT_RESULT,
    ) {
        let ctx = Box::from_raw(ctx as *mut Ctx<T>);

        threadpoolapiset::CloseThreadpoolWait(wait);

        (ctx.f)(ctx.evt);
    }

    let evt = assert_win32_nonnull(unsafe {
        CreateEventW(
            null_mut(),
            0,      // bManualReset
            0,      // bInitialState
            null(), //lpName
        )
    });

    let ctx = Box::new(Ctx { f, evt });

    let wait = assert_win32_nonnull(unsafe {
        threadpoolapiset::CreateThreadpoolWait(
            Some(handler::<T>),
            Box::into_raw(ctx) as _,
            null_mut(),
        )
    });

    unsafe { threadpoolapiset::SetThreadpoolWait(wait, evt, null_mut()) };

    evt
}

/// The compositor representation of a `Bitmap`. Stored in `Bitmap`.
pub(super) struct BitmapCompRepr {
    inner: MtLock<SetOnce<Box<BitmapCompReprInner>>>,
}

struct BitmapCompReprInner {
    surf: ComPtr<ICompositionSurface>,
    event_registration: RenderingDeviceReplacedEventRegistration,
}

// The referenced objects (`CompositionSurface`)
// are agile, thus can be accessed from any thread. `winrt-rust`'s code
// generator does not take agility into account at the moment.
//
// `MtSticky` would allow us to get rid of this, but instead
// would require the existence of a main thread at the point of dropping.
unsafe impl Send for BitmapCompReprInner {}

impl BitmapCompRepr {
    pub(super) fn new() -> Self {
        Self {
            inner: MtLock::new(SetOnce::empty()),
        }
    }
}

impl SurfaceMap {
    /// Get an `ICompositionSurface` for a given `Bitmap`. May cache the
    /// surface.
    pub fn get_surface_for_bitmap(&self, wm: Wm, bmp: &Bitmap) -> ComPtr<ICompositionSurface> {
        let inner_cell = bmp.inner.comp_repr.inner.get_with_wm(wm);

        if let Some(inner) = inner_cell.as_inner_ref() {
            return inner.surf.clone();
        }

        let inner = repeat_until_devlost_is_resolved(|| self.realize_bitmap(bmp));
        let surf = inner.surf.clone();
        let _ = inner_cell.store(Some(inner));
        surf
    }

    /// Construct a `BitmapCompReprInner` for a given `Bitmap`.
    fn realize_bitmap(
        &self,
        bmp: &Bitmap,
    ) -> Result<Box<BitmapCompReprInner>, DeviceLost> {
        use crate::iface::Bitmap;
        use std::convert::TryInto;
        let size = bmp.size();

        let winrt_size = SizeInt32 {
            Width: size[0].try_into().unwrap(),
            Height: size[1].try_into().unwrap(),
        };

        let cdsurf = self
            .comp_device2
            .create_drawing_surface2(
                winrt_size,
                DirectXPixelFormat::R8G8B8A8UIntNormalized,
                DirectXAlphaMode::Premultiplied,
            )
            .map_err(assert_winrt_devlost)?
            .unwrap();

        // TODO: Use `CompositionGraphicsDevice::RenderingDeviceReplaced`

        let cdsurf_interop: MyComPtr<ICompositionDrawingSurfaceInterop> =
            MyComPtr::iunknown_from_winrt_comptr(cdsurf.clone())
                .query_interface()
                .unwrap();

        // Retrieve a reference to the backing surface
        let (d2d_dc_cp, offset): (MyComPtr<ID2D1DeviceContext>, POINT) = unsafe {
            let mut out = MaybeUninit::uninit();
            let mut out_offset = MaybeUninit::uninit();
            assert_hresult_ok_or_devlost(cdsurf_interop.BeginDraw(
                null(),
                &ID2D1DeviceContext::uuidof(),
                out.as_mut_ptr() as _,
                out_offset.as_mut_ptr(),
            ))?;
            (
                MyComPtr::from_ptr_unchecked(out.assume_init()),
                out_offset.assume_init(),
            )
        };

        // Draw into the Direct2D DC
        {
            let mut d2d_dc = unsafe { direct2d::DeviceContext::from_raw(d2d_dc_cp.as_ptr()) };
            std::mem::forget(d2d_dc_cp); // ownership moved to `DeviceContext`

            d2d_dc.clear((0, 0.0));

            // Createa  `Bitmap` from `bmp`
            let in_bitmap_data = bmp.inner.lock();
            let in_bitmap_slice = unsafe {
                std::slice::from_raw_parts(
                    in_bitmap_data.as_ptr(),
                    in_bitmap_data.size()[1] as usize * in_bitmap_data.stride() as usize,
                )
            };

            let bitmap = direct2d::image::Bitmap::create(&d2d_dc)
                .with_raw_data(
                    SizeU(D2D_SIZE_U {
                        width: size[0],
                        height: size[1],
                    }),
                    in_bitmap_slice,
                    in_bitmap_data.stride(),
                )
                .with_format(dxgi::Format::B8G8R8A8Unorm)
                .with_alpha_mode(direct2d::enums::AlphaMode::Premultiplied)
                .build()
                .map_err(assert_d2d_devlost)?;

            // Draw the bitmap into the DC
            let in_rect = (0.0, 0.0, size[0] as f32, size[1] as f32);
            let out_rect = (
                offset.x as f32,
                offset.y as f32,
                (size[0] + offset.x as u32) as f32,
                (size[1] + offset.y as u32) as f32,
            );
            d2d_dc.draw_bitmap(
                &bitmap,
                out_rect,
                1.0,
                direct2d::enums::BitmapInterpolationMode::NearestNeighbor,
                in_rect,
            );
        }

        assert_hresult_ok_or_devlost(unsafe { cdsurf_interop.EndDraw() })?;

        let surf = cdsurf.query_interface().unwrap();

        Ok(Box::new(BitmapCompReprInner { surf }))
    }
}

/// Represents an error that will be resolved by recreating a DXGI device
/// and related objects.
struct DeviceLost(HRESULT);

fn assert_hresult_ok_or_devlost(e: HRESULT) -> Result<(), DeviceLost> {
    if e == 0 {
        Ok(())
    } else {
        Err(assert_hresult_devlost(e))
    }
}

/// Return `DeviceLost` if the error is recoverable. Panic otherwise.
/// `S_OK` is treated as an error.
fn assert_hresult_devlost(e: HRESULT) -> DeviceLost {
    use winapi::shared::winerror;
    match e {
        winerror::D2DERR_RECREATE_TARGET
        | winerror::DXGI_ERROR_DEVICE_REMOVED
        | winerror::DXGI_ERROR_DEVICE_RESET => DeviceLost(e),
        _ => panic_hresult(e),
    }
}

/// Return `DeviceLost` if the error is recoverable. Panic otherwise.
fn assert_d2d_devlost(e: direct2d::error::Error) -> DeviceLost {
    assert_hresult_devlost(e.into())
}

/// Return `DeviceLost` if the error is recoverable. Panic otherwise.
fn assert_winrt_devlost(e: winrt::Error) -> DeviceLost {
    assert_hresult_devlost(e.as_hresult())
}

/// Call the given closure repeatedly until it succeeds.
fn repeat_until_devlost_is_resolved<R>(mut f: impl FnMut() -> Result<R, DeviceLost>) -> R {
    use winapi::um::synchapi::Sleep;
    let mut wait = 1;
    let mut count = 10;

    loop {
        let err = match f() {
            Ok(r) => return r,
            Err(DeviceLost(err)) => err,
        };

        count -= 1;
        if count == 0 {
            log::error!(
                "Got HRESULT 0x{:08x}. Panicking - no attempts remaining",
                err
            );
            panic_hresult(err);
        } else {
            log::warn!(
                "Got HRESULT 0x{:08x}, which might have been caused by a \
                 'device lost' or similar condition. Retrying in {} \
                 millisecond(s)",
                err,
                wait
            );
        }

        // We watch for "device removed" events in a background thread, so
        // the error will be resolved if we wait for a bit
        unsafe { Sleep(wait) };

        // Exponential back-off with an upper bound of 4 seconds
        wait = (wait * 2 - 1) & 4095;
    }
}

/// Call the given closure repeatedly until it succeeds. On failure, it retries
/// the operation immediately without calling `Sleep`.
///
/// The reason `repeat_until_devlost_is_resolved` inserts a sleep is to wait
/// for the background thread to reinitialize the device object. However, if
/// the current thread is the background thread, then there is no point in
/// inserting a sleep. This method is suited for such a situation.
fn repeat_until_devlost_is_resolved_nodelay<R>(mut f: impl FnMut() -> Result<R, DeviceLost>) -> R {
    let mut count = 10;

    loop {
        let err = match f() {
            Ok(r) => return r,
            Err(DeviceLost(err)) => err,
        };

        count -= 1;
        if count == 0 {
            log::error!(
                "Got HRESULT 0x{:08x}. Panicking - no attempts remaining",
                err
            );
            panic_hresult(err);
        } else {
            log::warn!("Got HRESULT 0x{:08x}. Retrying", err);
        }
    }
}
