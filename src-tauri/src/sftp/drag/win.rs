//! Windows: an OLE drag of files (CF_HDROP) in a temporary folder. They are downloaded there
//! when the application they were dropped on asks for them after the drop (later, on a thread
//! of its own, when it supports asynchronous transfers as Explorer does), with messages pumped
//! meanwhile so that the window keeps working; that application then copies them.

use std::mem::ManuallyDrop;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::WebviewWindow;
use tokio::sync::oneshot;
use windows::core::{implement, Ref, BOOL, HRESULT};
use windows::Win32::Foundation::{
    DATA_S_SAMEFORMATETC, DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, DV_E_FORMATETC, E_FAIL,
    E_NOTIMPL, HGLOBAL, HWND, OLE_E_ADVISENOTSUPPORTED, POINT, S_OK,
};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::Com::{
    IAdviseSink, IBindCtx, IDataObject, IDataObject_Impl, IEnumFORMATETC, IEnumSTATDATA, DATADIR_GET, DVASPECT_CONTENT,
    FORMATETC, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GHND};
use windows::Win32::System::Ole::{DoDragDrop, IDropSource, IDropSource_Impl, CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE};
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Shell::{IDataObjectAsyncCapability, IDataObjectAsyncCapability_Impl, SHCreateStdEnumFmtEtc, DROPFILES};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetAncestor, GetCursorPos, MsgWaitForMultipleObjects, PeekMessageW, PostQuitMessage, TranslateMessage,
    WindowFromPoint, GA_ROOT, MSG, PM_REMOVE, QS_ALLINPUT, WM_QUIT,
};

use super::{DragResult, DropDownload, Item, Outcome};
use crate::error::{Error, Result};
use crate::sftp::edit::remove_old;

#[derive(Clone, Copy, PartialEq)]
enum Fetch {
    Idle,
    Running,
    Done { ok: bool },
}

/// Where and how the mouse button was released.
#[derive(Clone, Copy)]
struct Release {
    inside: bool,
    x: f64,
    y: f64,
}

struct Shared {
    /// Each remote item and where it goes in the temporary folder.
    items: Vec<(String, PathBuf)>,
    download: Arc<DropDownload>,
    window: isize,
    scale: f64,
    release: Mutex<Option<Release>>,
    /// Released over another window: the next request for the files is the drop's.
    dropped: AtomicBool,
    fetch: Mutex<Fetch>,
}

impl Shared {
    /// Downloads the items, once, pumping messages until done; whether that succeeded.
    fn fetch(self: &Arc<Self>) -> bool {
        let start = {
            let mut fetch = self.fetch.lock().unwrap();
            let idle = *fetch == Fetch::Idle;
            if idle {
                *fetch = Fetch::Running;
            }
            idle
        };
        if start {
            let this = self.clone();
            tauri::async_runtime::spawn(async move {
                let mut ok = true;
                for (remote, local) in &this.items {
                    ok &= this.download.fetch(remote, local.clone()).await.is_ok();
                }
                *this.fetch.lock().unwrap() = Fetch::Done { ok };
            });
        }
        loop {
            if let Fetch::Done { ok } = *self.fetch.lock().unwrap() {
                return ok;
            }
            pump_messages();
        }
    }
}

/// Handles the thread's messages for a moment, as a modal loop would.
fn pump_messages() {
    unsafe {
        MsgWaitForMultipleObjects(None, false, 50, QS_ALLINPUT);
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            if msg.message == WM_QUIT {
                // Left for the main loop.
                PostQuitMessage(msg.wParam.0 as i32);
                return;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn is_hdrop(format: &FORMATETC) -> bool {
    format.cfFormat == CF_HDROP.0 && format.dwAspect == DVASPECT_CONTENT.0 && format.tymed & TYMED_HGLOBAL.0 as u32 != 0
}

fn hdrop_format() -> FORMATETC {
    FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

/// A DROPFILES block listing `paths`.
fn drop_files<'a>(paths: impl Iterator<Item = &'a PathBuf>) -> windows::core::Result<HGLOBAL> {
    let mut names: Vec<u16> = Vec::new();
    for path in paths {
        names.extend(path.as_os_str().encode_wide());
        names.push(0);
    }
    names.push(0);
    let header = std::mem::size_of::<DROPFILES>();
    unsafe {
        let memory = GlobalAlloc(GHND, header + names.len() * 2)?;
        let base = GlobalLock(memory) as *mut u8;
        if base.is_null() {
            return Err(E_FAIL.into());
        }
        base.cast::<DROPFILES>().write_unaligned(DROPFILES {
            pFiles: header as u32,
            pt: POINT::default(),
            fNC: false.into(),
            fWide: true.into(),
        });
        std::ptr::copy_nonoverlapping(names.as_ptr().cast::<u8>(), base.add(header), names.len() * 2);
        // Fails once unlocked, which is the point.
        let _ = GlobalUnlock(memory);
        Ok(memory)
    }
}

#[implement(IDataObject, IDataObjectAsyncCapability)]
struct DataObject {
    shared: Arc<Shared>,
    async_mode: AtomicBool,
    in_operation: AtomicBool,
}

impl IDataObject_Impl for DataObject_Impl {
    fn GetData(&self, format: *const FORMATETC) -> windows::core::Result<STGMEDIUM> {
        let Some(format) = (unsafe { format.as_ref() }) else { return Err(DV_E_FORMATETC.into()) };
        if !is_hdrop(format) {
            return Err(DV_E_FORMATETC.into());
        }
        // Asked while hovering, the paths are enough; after the drop the files must be there.
        if self.shared.dropped.load(Ordering::Relaxed) && !self.shared.fetch() {
            return Err(E_FAIL.into());
        }
        let memory = drop_files(self.shared.items.iter().map(|(_, local)| local))?;
        Ok(STGMEDIUM {
            tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 { hGlobal: memory },
            pUnkForRelease: ManuallyDrop::new(None),
        })
    }

    fn GetDataHere(&self, _format: *const FORMATETC, _medium: *mut STGMEDIUM) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn QueryGetData(&self, format: *const FORMATETC) -> HRESULT {
        match unsafe { format.as_ref() } {
            Some(format) if is_hdrop(format) => S_OK,
            _ => DV_E_FORMATETC,
        }
    }

    fn GetCanonicalFormatEtc(&self, _format: *const FORMATETC, out: *mut FORMATETC) -> HRESULT {
        if let Some(out) = unsafe { out.as_mut() } {
            out.ptd = std::ptr::null_mut();
        }
        DATA_S_SAMEFORMATETC
    }

    fn SetData(&self, _format: *const FORMATETC, _medium: *const STGMEDIUM, _release: BOOL) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn EnumFormatEtc(&self, direction: u32) -> windows::core::Result<IEnumFORMATETC> {
        if direction != DATADIR_GET.0 as u32 {
            return Err(E_NOTIMPL.into());
        }
        unsafe { SHCreateStdEnumFmtEtc(&[hdrop_format()]) }
    }

    fn DAdvise(&self, _format: *const FORMATETC, _advf: u32, _sink: Ref<IAdviseSink>) -> windows::core::Result<u32> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }

    fn DUnadvise(&self, _connection: u32) -> windows::core::Result<()> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }

    fn EnumDAdvise(&self) -> windows::core::Result<IEnumSTATDATA> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }
}

// Lets Explorer take the files on a thread of its own after the drop, instead of in the drop
// itself, so that neither its window nor ours waits for the download.
impl IDataObjectAsyncCapability_Impl for DataObject_Impl {
    fn SetAsyncMode(&self, async_mode: BOOL) -> windows::core::Result<()> {
        self.async_mode.store(async_mode.as_bool(), Ordering::Relaxed);
        Ok(())
    }

    fn GetAsyncMode(&self) -> windows::core::Result<BOOL> {
        Ok(self.async_mode.load(Ordering::Relaxed).into())
    }

    fn StartOperation(&self, _context: Ref<IBindCtx>) -> windows::core::Result<()> {
        self.in_operation.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn InOperation(&self) -> windows::core::Result<BOOL> {
        Ok(self.in_operation.load(Ordering::Relaxed).into())
    }

    fn EndOperation(&self, _result: HRESULT, _context: Ref<IBindCtx>, _effects: u32) -> windows::core::Result<()> {
        self.in_operation.store(false, Ordering::Relaxed);
        Ok(())
    }
}

#[implement(IDropSource)]
struct DropSource {
    shared: Arc<Shared>,
}

impl IDropSource_Impl for DropSource_Impl {
    fn QueryContinueDrag(&self, escape: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
        if escape.as_bool() {
            return DRAGDROP_S_CANCEL;
        }
        if keys.0 & MK_LBUTTON.0 != 0 {
            return S_OK;
        }
        let window = HWND(self.shared.window as *mut _);
        let mut point = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut point);
            let inside = GetAncestor(WindowFromPoint(point), GA_ROOT) == window;
            let _ = ScreenToClient(window, &mut point);
            let scale = self.shared.scale;
            *self.shared.release.lock().unwrap() =
                Some(Release { inside, x: f64::from(point.x) / scale, y: f64::from(point.y) / scale });
            self.shared.dropped.store(!inside, Ordering::Relaxed);
        }
        DRAGDROP_S_DROP
    }

    fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

/// Runs the drag on the main thread (`DoDragDrop` returns once it is over); `done` gets how it
/// ended.
pub(super) fn start(
    window: &WebviewWindow,
    items: Vec<Item>,
    download: Arc<DropDownload>,
    done: oneshot::Sender<DragResult>,
) -> Result<()> {
    let hwnd = window.hwnd().map_err(Error::from)?.0 as isize;
    let scale = window.scale_factor().map_err(Error::from)?;
    let root = std::env::temp_dir().join("ZShell-drag");
    remove_old(&root);
    let dir = root.join(&download.transfer_id);
    let items: Vec<_> = items.iter().map(|item| (item.path.clone(), dir.join(item.name()), item.is_dir)).collect();
    // Placeholders, in case the application dragged over looks at the files before the drop.
    for (_, local, is_dir) in &items {
        let _ = if *is_dir { std::fs::create_dir_all(local) } else { std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(local, b"")) };
    }
    let items = items.into_iter().map(|(remote, local, _)| (remote, local)).collect();
    let shared = Arc::new(Shared {
        items,
        download,
        window: hwnd,
        scale,
        release: Mutex::new(None),
        dropped: AtomicBool::new(false),
        fetch: Mutex::new(Fetch::Idle),
    });
    window
        .run_on_main_thread(move || {
            let data: IDataObject =
                DataObject { shared: shared.clone(), async_mode: AtomicBool::new(true), in_operation: AtomicBool::new(false) }
                    .into();
            let source: IDropSource = DropSource { shared: shared.clone() }.into();
            let mut effect = DROPEFFECT_NONE;
            let hr = unsafe { DoDragDrop(&data, &source, DROPEFFECT_COPY, &mut effect) };
            let release = *shared.release.lock().unwrap();
            let result = match release {
                Some(Release { inside, x, y }) if hr == DRAGDROP_S_DROP => {
                    DragResult { outcome: if inside { Outcome::Inside } else { Outcome::Outside }, x, y }
                }
                _ => DragResult::cancelled(),
            };
            let _ = done.send(result);
        })
        .map_err(Error::from)
}
