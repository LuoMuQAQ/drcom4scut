//! WebView2 现代 UI 宿主绑定。
//! 支持内联资源渲染、受控双向 IPC 消息、外链系统浏览器拦截和安全加载。
#![allow(dead_code, unused_variables)]

pub mod com;
pub mod loader;

use std::ffi::c_void;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use windows::core::{w, BOOL, HRESULT, Interface, IUnknown, PCWSTR};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, DestroyWindow, GetClientRect, PostMessageW, SW_SHOWNORMAL, WM_APP,
};

pub use loader::{detect_runtime, RuntimeStatus};

pub const WM_WEBVIEW_MESSAGE: u32 = WM_APP + 50;

#[derive(Debug)]
pub enum WebViewError {
    RuntimeMissing,
    LoaderFailed(String),
    EnvironmentCreationFailed(HRESULT),
    ControllerCreationFailed(HRESULT),
    NavigationFailed(HRESULT),
}

impl std::fmt::Display for WebViewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RuntimeMissing => write!(f, "未检测到 Microsoft Edge WebView2 运行时。"),
            Self::LoaderFailed(e) => write!(f, "无法加载 WebView2Loader: {e}"),
            Self::EnvironmentCreationFailed(hr) => write!(f, "创建 WebView2 环境失败: {hr:?}"),
            Self::ControllerCreationFailed(hr) => write!(f, "创建 WebView2 控制器失败: {hr:?}"),
            Self::NavigationFailed(hr) => write!(f, "WebView2 导航页面失败: {hr:?}"),
        }
    }
}

impl std::error::Error for WebViewError {}

type MessageCallback = Box<dyn Fn(&str) + Send + Sync + 'static>;

pub struct WebView {
    controller: com::ICoreWebView2Controller,
    webview: com::ICoreWebView2,
    _msg_rx: Option<Receiver<String>>,
}

unsafe impl Send for WebView {}
unsafe impl Sync for WebView {}

// Handler implementations using COM vtables
#[repr(C)]
struct EnvCompletedHandler {
    vtbl: *const EnvCompletedHandlerVtbl,
    ref_count: std::sync::atomic::AtomicU32,
    tx: Sender<Result<com::ICoreWebView2Environment, HRESULT>>,
}

#[repr(C)]
struct EnvCompletedHandlerVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const windows::core::GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    invoke: unsafe extern "system" fn(*mut c_void, HRESULT, *mut c_void) -> HRESULT,
}

static ENV_HANDLER_VTBL: EnvCompletedHandlerVtbl = EnvCompletedHandlerVtbl {
    query_interface: env_query_interface,
    add_ref: env_add_ref,
    release: env_release,
    invoke: env_invoke,
};

unsafe extern "system" fn env_query_interface(this: *mut c_void, iid: *const windows::core::GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return windows::core::HRESULT(0x80004003u32 as i32); // E_POINTER
    }
    if *iid == IUnknown::IID || *iid == com::IID_ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler {
        *out = this;
        env_add_ref(this);
        windows::core::HRESULT(0)
    } else {
        *out = std::ptr::null_mut();
        windows::core::HRESULT(0x80004002u32 as i32) // E_NOINTERFACE
    }
}

unsafe extern "system" fn env_add_ref(this: *mut c_void) -> u32 {
    let handler = &*(this as *mut EnvCompletedHandler);
    handler.ref_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
}

unsafe extern "system" fn env_release(this: *mut c_void) -> u32 {
    let handler = &*(this as *mut EnvCompletedHandler);
    let count = handler.ref_count.fetch_sub(1, std::sync::atomic::Ordering::Release) - 1;
    if count == 0 {
        std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
        drop(Box::from_raw(this as *mut EnvCompletedHandler));
    }
    count
}

unsafe extern "system" fn env_invoke(this: *mut c_void, result: HRESULT, env: *mut c_void) -> HRESULT {
    let handler = &*(this as *mut EnvCompletedHandler);
    if result.0 >= 0 && !env.is_null() {
        let env_obj: com::ICoreWebView2Environment = std::mem::transmute(env);
        // Note: env COM object was already AddRef'ed by caller
        let _ = handler.tx.send(Ok(env_obj));
    } else {
        let _ = handler.tx.send(Err(result));
    }
    windows::core::HRESULT(0)
}

// Controller Handler
#[repr(C)]
struct ControllerCompletedHandler {
    vtbl: *const ControllerCompletedHandlerVtbl,
    ref_count: std::sync::atomic::AtomicU32,
    tx: Sender<Result<com::ICoreWebView2Controller, HRESULT>>,
}

#[repr(C)]
struct ControllerCompletedHandlerVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const windows::core::GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    invoke: unsafe extern "system" fn(*mut c_void, HRESULT, *mut c_void) -> HRESULT,
}

static CONTROLLER_HANDLER_VTBL: ControllerCompletedHandlerVtbl = ControllerCompletedHandlerVtbl {
    query_interface: controller_query_interface,
    add_ref: controller_add_ref,
    release: controller_release,
    invoke: controller_invoke,
};

unsafe extern "system" fn controller_query_interface(this: *mut c_void, iid: *const windows::core::GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return windows::core::HRESULT(0x80004003u32 as i32);
    }
    if *iid == IUnknown::IID || *iid == com::IID_ICoreWebView2CreateCoreWebView2ControllerCompletedHandler {
        *out = this;
        controller_add_ref(this);
        windows::core::HRESULT(0)
    } else {
        *out = std::ptr::null_mut();
        windows::core::HRESULT(0x80004002u32 as i32)
    }
}

unsafe extern "system" fn controller_add_ref(this: *mut c_void) -> u32 {
    let handler = &*(this as *mut ControllerCompletedHandler);
    handler.ref_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
}

unsafe extern "system" fn controller_release(this: *mut c_void) -> u32 {
    let handler = &*(this as *mut ControllerCompletedHandler);
    let count = handler.ref_count.fetch_sub(1, std::sync::atomic::Ordering::Release) - 1;
    if count == 0 {
        std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
        drop(Box::from_raw(this as *mut ControllerCompletedHandler));
    }
    count
}

unsafe extern "system" fn controller_invoke(this: *mut c_void, result: HRESULT, controller: *mut c_void) -> HRESULT {
    let handler = &*(this as *mut ControllerCompletedHandler);
    if result.0 >= 0 && !controller.is_null() {
        let ctrl_obj: com::ICoreWebView2Controller = std::mem::transmute(controller);
        let _ = handler.tx.send(Ok(ctrl_obj));
    } else {
        let _ = handler.tx.send(Err(result));
    }
    windows::core::HRESULT(0)
}

// WebMessage Handler
#[repr(C)]
struct WebMessageHandler {
    vtbl: *const WebMessageHandlerVtbl,
    ref_count: std::sync::atomic::AtomicU32,
    callback: Arc<Mutex<Option<MessageCallback>>>,
}

#[repr(C)]
struct WebMessageHandlerVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const windows::core::GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    invoke: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) -> HRESULT,
}

static WEB_MESSAGE_HANDLER_VTBL: WebMessageHandlerVtbl = WebMessageHandlerVtbl {
    query_interface: web_msg_query_interface,
    add_ref: web_msg_add_ref,
    release: web_msg_release,
    invoke: web_msg_invoke,
};

unsafe extern "system" fn web_msg_query_interface(this: *mut c_void, iid: *const windows::core::GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return windows::core::HRESULT(0x80004003u32 as i32);
    }
    if *iid == IUnknown::IID || *iid == com::IID_ICoreWebView2WebMessageReceivedEventHandler {
        *out = this;
        web_msg_add_ref(this);
        windows::core::HRESULT(0)
    } else {
        *out = std::ptr::null_mut();
        windows::core::HRESULT(0x80004002u32 as i32)
    }
}

unsafe extern "system" fn web_msg_add_ref(this: *mut c_void) -> u32 {
    let handler = &*(this as *mut WebMessageHandler);
    handler.ref_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
}

unsafe extern "system" fn web_msg_release(this: *mut c_void) -> u32 {
    let handler = &*(this as *mut WebMessageHandler);
    let count = handler.ref_count.fetch_sub(1, std::sync::atomic::Ordering::Release) - 1;
    if count == 0 {
        std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
        drop(Box::from_raw(this as *mut WebMessageHandler));
    }
    count
}

unsafe extern "system" fn web_msg_invoke(this: *mut c_void, _sender: *mut c_void, args: *mut c_void) -> HRESULT {
    let handler = &*(this as *mut WebMessageHandler);
    if !args.is_null() {
        let args_obj: &com::ICoreWebView2WebMessageReceivedEventArgs = &*(args as *const _);
        let mut msg_ptr: *mut u16 = std::ptr::null_mut();
        let hr = args_obj.GetWebMessageAsJson(&mut msg_ptr);
        if hr.0 >= 0 && !msg_ptr.is_null() {
            let mut len = 0;
            while *msg_ptr.add(len) != 0 {
                len += 1;
            }
            let slice = std::slice::from_raw_parts(msg_ptr, len);
            let json_str = String::from_utf16_lossy(slice);
            windows::Win32::System::Com::CoTaskMemFree(Some(msg_ptr as *mut _));

            if let Ok(guard) = handler.callback.lock() {
                if let Some(cb) = guard.as_ref() {
                    cb(&json_str);
                }
            }
        }
    }
    windows::core::HRESULT(0)
}

// Navigation Starting Handler (阻止任何外部导航，外链调用默认浏览器)
#[repr(C)]
struct NavigationStartingHandler {
    vtbl: *const NavigationStartingHandlerVtbl,
    ref_count: std::sync::atomic::AtomicU32,
}

#[repr(C)]
struct NavigationStartingHandlerVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const windows::core::GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    invoke: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) -> HRESULT,
}

static NAV_HANDLER_VTBL: NavigationStartingHandlerVtbl = NavigationStartingHandlerVtbl {
    query_interface: nav_query_interface,
    add_ref: nav_add_ref,
    release: nav_release,
    invoke: nav_invoke,
};

unsafe extern "system" fn nav_query_interface(this: *mut c_void, iid: *const windows::core::GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return windows::core::HRESULT(0x80004003u32 as i32);
    }
    if *iid == IUnknown::IID || *iid == com::IID_ICoreWebView2NavigationStartingEventHandler {
        *out = this;
        nav_add_ref(this);
        windows::core::HRESULT(0)
    } else {
        *out = std::ptr::null_mut();
        windows::core::HRESULT(0x80004002u32 as i32)
    }
}

unsafe extern "system" fn nav_add_ref(this: *mut c_void) -> u32 {
    let handler = &*(this as *mut NavigationStartingHandler);
    handler.ref_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
}

unsafe extern "system" fn nav_release(this: *mut c_void) -> u32 {
    let handler = &*(this as *mut NavigationStartingHandler);
    let count = handler.ref_count.fetch_sub(1, std::sync::atomic::Ordering::Release) - 1;
    if count == 0 {
        std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
        drop(Box::from_raw(this as *mut NavigationStartingHandler));
    }
    count
}

unsafe extern "system" fn nav_invoke(this: *mut c_void, _sender: *mut c_void, args: *mut c_void) -> HRESULT {
    if !args.is_null() {
        let args_obj: &com::ICoreWebView2NavigationStartingEventArgs = &*(args as *const _);
        let mut uri_ptr: *mut u16 = std::ptr::null_mut();
        let hr = args_obj.GetUri(&mut uri_ptr);
        if hr.0 >= 0 && !uri_ptr.is_null() {
            let mut len = 0;
            while *uri_ptr.add(len) != 0 {
                len += 1;
            }
            let slice = std::slice::from_raw_parts(uri_ptr, len);
            let uri = String::from_utf16_lossy(slice);
            windows::Win32::System::Com::CoTaskMemFree(Some(uri_ptr as *mut _));

            // 如果不是内部 about:blank 或初始页面，则取消导航并转交给系统浏览器
            if !uri.starts_with("data:") && uri != "about:blank" {
                let _ = args_obj.PutCancel(BOOL(1));
                let wide_uri: Vec<u16> = uri.encode_utf16().chain(std::iter::once(0)).collect();
                let _ = ShellExecuteW(
                    None,
                    w!("open"),
                    PCWSTR(wide_uri.as_ptr()),
                    None,
                    None,
                    SW_SHOWNORMAL,
                );
            }
        }
    }
    windows::core::HRESULT(0)
}

impl WebView {
    /// 在指定的宿主 HWND 内创建并嵌入 WebView2 实例。
    pub fn create<F>(
        parent: HWND,
        html_content: &str,
        on_message: F,
    ) -> Result<Self, WebViewError>
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }

        // 1. 动态获取 Loader
        let create_env_fn = loader::ensure_loader().map_err(WebViewError::LoaderFailed)?;

        // 2. 准备用户数据目录（隔离在 LocalAppData 独立目录下）
        let local_data = std::env::var_os("LOCALAPPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(r"C:\ProgramData"))
            .join("drcom4scut")
            .join("webview-data");
        let _ = std::fs::create_dir_all(&local_data);

        let data_dir_w: Vec<u16> = local_data.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();

        // 3. 异步创建 Environment
        let (env_tx, env_rx) = channel();
        let env_handler = Box::into_raw(Box::new(EnvCompletedHandler {
            vtbl: &ENV_HANDLER_VTBL,
            ref_count: std::sync::atomic::AtomicU32::new(1),
            tx: env_tx,
        })) as *mut c_void;

        let hr = unsafe {
            create_env_fn(
                PCWSTR::null(),
                PCWSTR(data_dir_w.as_ptr()),
                std::ptr::null_mut(),
                env_handler,
            )
        };
        if hr.0 < 0 {
            return Err(WebViewError::EnvironmentCreationFailed(hr));
        }

        // 驱动 Win32 消息循环等待环境就绪
        let environment = pump_until(env_rx).map_err(WebViewError::EnvironmentCreationFailed)?;

        // 4. 创建 Controller
        let (ctrl_tx, ctrl_rx) = channel();
        let ctrl_handler = Box::into_raw(Box::new(ControllerCompletedHandler {
            vtbl: &CONTROLLER_HANDLER_VTBL,
            ref_count: std::sync::atomic::AtomicU32::new(1),
            tx: ctrl_tx,
        })) as *mut c_void;

        let hr = unsafe { environment.CreateCoreWebView2Controller(parent, ctrl_handler) };
        if hr.0 < 0 {
            return Err(WebViewError::ControllerCreationFailed(hr));
        }

        let controller = pump_until(ctrl_rx).map_err(WebViewError::ControllerCreationFailed)?;

        // 5. 获取 CoreWebView2 接口
        let mut webview_raw: *mut c_void = std::ptr::null_mut();
        let hr = unsafe { controller.GetCoreWebView2(&mut webview_raw) };
        if hr.0 < 0 || webview_raw.is_null() {
            return Err(WebViewError::ControllerCreationFailed(hr));
        }
        let webview: com::ICoreWebView2 = unsafe { std::mem::transmute(webview_raw) };

        // 6. 配置 Settings（禁用右键菜单、状态栏；启用脚本和 WebMessage）
        let mut settings_raw: *mut c_void = std::ptr::null_mut();
        if unsafe { webview.GetSettings(&mut settings_raw).0 } >= 0 && !settings_raw.is_null() {
            let settings: &com::ICoreWebView2Settings = unsafe { &*(settings_raw as *const _) };
            unsafe {
                let _ = settings.PutIsScriptEnabled(BOOL(1));
                let _ = settings.PutIsWebMessageEnabled(BOOL(1));
                let _ = settings.PutAreDefaultContextMenusEnabled(BOOL(0));
                let _ = settings.PutIsStatusBarEnabled(BOOL(0));
                let _ = settings.PutAreDevToolsEnabled(BOOL(0));
                let _ = settings.PutIsZoomControlEnabled(BOOL(0));
                let _ = settings.PutIsBuiltInErrorPageEnabled(BOOL(0));
            }
        }

        // 7. 设置尺寸匹配宿主窗口
        let mut client_rect = RECT::default();
        unsafe {
            let _ = GetClientRect(parent, &mut client_rect);
            let _ = controller.PutBounds(client_rect);
            let _ = controller.PutIsVisible(BOOL(1));
        }

        // 8. 注册 WebMessageReceived 监听
        let msg_handler = Box::into_raw(Box::new(WebMessageHandler {
            vtbl: &WEB_MESSAGE_HANDLER_VTBL,
            ref_count: std::sync::atomic::AtomicU32::new(1),
            callback: Arc::new(Mutex::new(Some(Box::new(on_message)))),
        })) as *mut c_void;
        let mut msg_token: i64 = 0;
        unsafe {
            let _ = webview.AddWebMessageReceived(msg_handler, &mut msg_token);
        }

        // 9. 注册 NavigationStarting 拦截外部链接
        let nav_handler = Box::into_raw(Box::new(NavigationStartingHandler {
            vtbl: &NAV_HANDLER_VTBL,
            ref_count: std::sync::atomic::AtomicU32::new(1),
        })) as *mut c_void;
        let mut nav_token: i64 = 0;
        unsafe {
            let _ = webview.AddNavigationStarting(nav_handler, &mut nav_token);
        }

        // 10. 载入内联 HTML
        let html_w: Vec<u16> = html_content.encode_utf16().chain(std::iter::once(0)).collect();
        let hr = unsafe { webview.NavigateToString(PCWSTR(html_w.as_ptr())) };
        if hr.0 < 0 {
            return Err(WebViewError::NavigationFailed(hr));
        }

        Ok(Self {
            controller,
            webview,
            _msg_rx: None,
        })
    }

    /// 向 WebView2 前端发送 JSON 格式事件消息。
    pub fn post_message(&self, json: &str) -> Result<(), WebViewError> {
        let json_w: Vec<u16> = json.encode_utf16().chain(std::iter::once(0)).collect();
        let hr = unsafe { self.webview.PostWebMessageAsJson(PCWSTR(json_w.as_ptr())) };
        if hr.0 < 0 {
            Err(WebViewError::NavigationFailed(hr))
        } else {
            Ok(())
        }
    }

    /// 同步调整 WebView2 控件边界尺寸。
    pub fn resize(&self, bounds: RECT) {
        unsafe {
            let _ = self.controller.PutBounds(bounds);
            let _ = self.controller.NotifyParentWindowPositionChanged();
        }
    }

    /// 设置可见性。
    pub fn set_visible(&self, visible: bool) {
        unsafe {
            let _ = self.controller.PutIsVisible(BOOL(if visible { 1 } else { 0 }));
        }
    }
}

impl Drop for WebView {
    fn drop(&mut self) {
        unsafe {
            let _ = self.controller.Close();
        }
    }
}

/// 消息循环泵送等待直到 receiver 收到结果，保证 COM 异步回调在当前 STA 线程正常调度。
fn pump_until<T>(rx: Receiver<Result<T, HRESULT>>) -> Result<T, HRESULT> {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MsgWaitForMultipleObjects, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, QS_ALLINPUT,
    };
    loop {
        if let Ok(res) = rx.try_recv() {
            return res;
        }
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if let Ok(res) = rx.try_recv() {
                return res;
            }
            let _ = MsgWaitForMultipleObjects(None, false, 20, QS_ALLINPUT);
        }
    }
}
