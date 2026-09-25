//! WebView2 COM 接口定义与事件处理。
#![allow(non_snake_case, non_camel_case_types, dead_code)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
use windows::core::{BOOL, GUID, HRESULT, PCWSTR};
use windows::core::{IUnknown, IUnknown_Vtbl};
use windows::Win32::Foundation::{HWND, RECT};

// COM GUIDs
pub const IID_ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler: GUID =
    GUID::from_u128(0x2f314966_b83c_44f2_9515_32e6a3240578);
pub const IID_ICoreWebView2CreateCoreWebView2ControllerCompletedHandler: GUID =
    GUID::from_u128(0xe13e0c70_23a6_4444_8cb3_ff5d71c85d74);
pub const IID_ICoreWebView2WebMessageReceivedEventHandler: GUID =
    GUID::from_u128(0x5720e806_340d_4059_86c3_1952213824e1);
pub const IID_ICoreWebView2NavigationStartingEventHandler: GUID =
    GUID::from_u128(0x928de272_7494_430b_abea_790e21e24712);

pub const IID_ICoreWebView2Environment: GUID =
    GUID::from_u128(0xb96d755e_0319_4e92_a296_23436f46a1fc);
pub const IID_ICoreWebView2Controller: GUID =
    GUID::from_u128(0x4d00c0d3_0fb8_406e_8bb4_9846e3cf4588);
pub const IID_ICoreWebView2: GUID =
    GUID::from_u128(0x76ece415_5aa8_4e83_8a34_fb3cb50c73d9);
pub const IID_ICoreWebView2Settings: GUID =
    GUID::from_u128(0xe562e4f0_d7fa_43ac_8d71_c05150499f00);
pub const IID_ICoreWebView2WebMessageReceivedEventArgs: GUID =
    GUID::from_u128(0x0fcf683a_93d1_4476_8440_9063622416b9);
pub const IID_ICoreWebView2NavigationStartingEventArgs: GUID =
    GUID::from_u128(0x5b8649b9_4047_4d99_8bb6_01c277717d23);

#[windows::core::interface("b96d755e-0319-4e92-a296-23436f46a1fc")]
pub unsafe trait ICoreWebView2Environment: IUnknown {
    pub unsafe fn CreateCoreWebView2Controller(
        &self,
        parentwindow: HWND,
        handler: *mut c_void,
    ) -> HRESULT;
    pub unsafe fn CreateWebResourceResponse(
        &self,
        content: *mut c_void,
        statuscode: i32,
        reasonphrase: PCWSTR,
        headers: PCWSTR,
        response: *mut *mut c_void,
    ) -> HRESULT;
    pub unsafe fn GetBrowserVersionString(&self, versioninfo: *mut *mut u16) -> HRESULT;
    pub unsafe fn AddNewBrowserVersionAvailable(&self, handler: *mut c_void, token: *mut i64) -> HRESULT;
    pub unsafe fn RemoveNewBrowserVersionAvailable(&self, token: i64) -> HRESULT;
}

#[windows::core::interface("4d00c0d3-0fb8-406e-8bb4-9846e3cf4588")]
pub unsafe trait ICoreWebView2Controller: IUnknown {
    pub unsafe fn GetIsVisible(&self, isvisible: *mut BOOL) -> HRESULT;
    pub unsafe fn PutIsVisible(&self, isvisible: BOOL) -> HRESULT;
    pub unsafe fn GetBounds(&self, bounds: *mut RECT) -> HRESULT;
    pub unsafe fn PutBounds(&self, bounds: RECT) -> HRESULT;
    pub unsafe fn GetZoomFactor(&self, zoomfactor: *mut f64) -> HRESULT;
    pub unsafe fn PutZoomFactor(&self, zoomfactor: f64) -> HRESULT;
    pub unsafe fn AddZoomFactorChanged(&self, handler: *mut c_void, token: *mut i64) -> HRESULT;
    pub unsafe fn RemoveZoomFactorChanged(&self, token: i64) -> HRESULT;
    pub unsafe fn SetBoundsAndZoomFactor(&self, bounds: RECT, zoomfactor: f64) -> HRESULT;
    pub unsafe fn MoveFocus(&self, reason: i32) -> HRESULT;
    pub unsafe fn AddMoveFocusRequested(&self, handler: *mut c_void, token: *mut i64) -> HRESULT;
    pub unsafe fn RemoveMoveFocusRequested(&self, token: i64) -> HRESULT;
    pub unsafe fn AddGotFocus(&self, handler: *mut c_void, token: *mut i64) -> HRESULT;
    pub unsafe fn RemoveGotFocus(&self, token: i64) -> HRESULT;
    pub unsafe fn AddLostFocus(&self, handler: *mut c_void, token: *mut i64) -> HRESULT;
    pub unsafe fn RemoveLostFocus(&self, token: i64) -> HRESULT;
    pub unsafe fn AddAcceleratorKeyPressed(&self, handler: *mut c_void, token: *mut i64) -> HRESULT;
    pub unsafe fn RemoveAcceleratorKeyPressed(&self, token: i64) -> HRESULT;
    pub unsafe fn GetParentWindow(&self, toplevelwindow: *mut HWND) -> HRESULT;
    pub unsafe fn PutParentWindow(&self, toplevelwindow: HWND) -> HRESULT;
    pub unsafe fn NotifyParentWindowPositionChanged(&self) -> HRESULT;
    pub unsafe fn Close(&self) -> HRESULT;
    pub unsafe fn GetCoreWebView2(&self, corewebview2: *mut *mut c_void) -> HRESULT;
}

#[windows::core::interface("76ece415-5aa8-4e83-8a34-fb3cb50c73d9")]
pub unsafe trait ICoreWebView2: IUnknown {
    pub unsafe fn GetSettings(&self, settings: *mut *mut c_void) -> HRESULT;
    pub unsafe fn GetSource(&self, uri: *mut *mut u16) -> HRESULT;
    pub unsafe fn Navigate(&self, uri: PCWSTR) -> HRESULT;
    pub unsafe fn NavigateToString(&self, htmlcontent: PCWSTR) -> HRESULT;
    pub unsafe fn AddNavigationStarting(&self, handler: *mut c_void, token: *mut i64) -> HRESULT;
    pub unsafe fn RemoveNavigationStarting(&self, token: i64) -> HRESULT;
    pub unsafe fn AddNavigationCompleted(&self, handler: *mut c_void, token: *mut i64) -> HRESULT;
    pub unsafe fn RemoveNavigationCompleted(&self, token: i64) -> HRESULT;
    pub unsafe fn AddFrameNavigationStarting(&self, handler: *mut c_void, token: *mut i64) -> HRESULT;
    pub unsafe fn RemoveFrameNavigationStarting(&self, token: i64) -> HRESULT;
    pub unsafe fn AddFrameNavigationCompleted(&self, handler: *mut c_void, token: *mut i64) -> HRESULT;
    pub unsafe fn RemoveFrameNavigationCompleted(&self, token: i64) -> HRESULT;
    pub unsafe fn AddSourceChanged(&self, handler: *mut c_void, token: *mut i64) -> HRESULT;
    pub unsafe fn RemoveSourceChanged(&self, token: i64) -> HRESULT;
    pub unsafe fn AddHistoryChanged(&self, handler: *mut c_void, token: *mut i64) -> HRESULT;
    pub unsafe fn RemoveHistoryChanged(&self, token: i64) -> HRESULT;
    pub unsafe fn AddContentLoading(&self, handler: *mut c_void, token: *mut i64) -> HRESULT;
    pub unsafe fn RemoveContentLoading(&self, token: i64) -> HRESULT;
    pub unsafe fn AddDocumentTitleChanged(&self, handler: *mut c_void, token: *mut i64) -> HRESULT;
    pub unsafe fn RemoveDocumentTitleChanged(&self, token: i64) -> HRESULT;
    pub unsafe fn AddNewWindowRequested(&self, handler: *mut c_void, token: *mut i64) -> HRESULT;
    pub unsafe fn RemoveNewWindowRequested(&self, token: i64) -> HRESULT;
    pub unsafe fn AddDocumentCreated(&self, handler: *mut c_void, token: *mut i64) -> HRESULT;
    pub unsafe fn RemoveDocumentCreated(&self, token: i64) -> HRESULT;
    pub unsafe fn AddWebMessageReceived(&self, handler: *mut c_void, token: *mut i64) -> HRESULT;
    pub unsafe fn RemoveWebMessageReceived(&self, token: i64) -> HRESULT;
    pub unsafe fn PostWebMessageAsJson(&self, webmessageasjson: PCWSTR) -> HRESULT;
    pub unsafe fn PostWebMessageAsString(&self, webmessageasstring: PCWSTR) -> HRESULT;
    pub unsafe fn AddWindowCloseRequested(&self, handler: *mut c_void, token: *mut i64) -> HRESULT;
    pub unsafe fn RemoveWindowCloseRequested(&self, token: i64) -> HRESULT;
    pub unsafe fn AddProcessFailed(&self, handler: *mut c_void, token: *mut i64) -> HRESULT;
    pub unsafe fn RemoveProcessFailed(&self, token: i64) -> HRESULT;
    pub unsafe fn AddScriptDialogOpening(&self, handler: *mut c_void, token: *mut i64) -> HRESULT;
    pub unsafe fn RemoveScriptDialogOpening(&self, token: i64) -> HRESULT;
    pub unsafe fn AddPermissionRequested(&self, handler: *mut c_void, token: *mut i64) -> HRESULT;
    pub unsafe fn RemovePermissionRequested(&self, token: i64) -> HRESULT;
    pub unsafe fn ExecuteScript(&self, javascript: PCWSTR, handler: *mut c_void) -> HRESULT;
    pub unsafe fn CapturePreview(&self, format: i32, imagestream: *mut c_void, handler: *mut c_void) -> HRESULT;
    pub unsafe fn Reload(&self) -> HRESULT;
    pub unsafe fn PostWebMessageAsJson2(&self, webmessageasjson: PCWSTR, additionalallowedorigins: *mut c_void) -> HRESULT;
    pub unsafe fn PostWebMessageAsString2(&self, webmessageasstring: PCWSTR, additionalallowedorigins: *mut c_void) -> HRESULT;
    pub unsafe fn GetBrowserProcessId(&self, value: *mut u32) -> HRESULT;
    pub unsafe fn GetCanGoBack(&self, cangoBack: *mut BOOL) -> HRESULT;
    pub unsafe fn GetCanGoForward(&self, cangoForward: *mut BOOL) -> HRESULT;
    pub unsafe fn GoBack(&self) -> HRESULT;
    pub unsafe fn GoForward(&self) -> HRESULT;
    pub unsafe fn GetDevToolsProtocolEventReceiver(&self, eventname: PCWSTR, receiver: *mut *mut c_void) -> HRESULT;
    pub unsafe fn Stop(&self) -> HRESULT;
}

#[windows::core::interface("e562e4f0-d7fa-43ac-8d71-c05150499f00")]
pub unsafe trait ICoreWebView2Settings: IUnknown {
    pub unsafe fn GetIsScriptEnabled(&self, isscriptenabled: *mut BOOL) -> HRESULT;
    pub unsafe fn PutIsScriptEnabled(&self, isscriptenabled: BOOL) -> HRESULT;
    pub unsafe fn GetIsWebMessageEnabled(&self, iswebmessageenabled: *mut BOOL) -> HRESULT;
    pub unsafe fn PutIsWebMessageEnabled(&self, iswebmessageenabled: BOOL) -> HRESULT;
    pub unsafe fn GetAreDefaultScriptDialogsEnabled(&self, aredefaultscriptdialogsenabled: *mut BOOL) -> HRESULT;
    pub unsafe fn PutAreDefaultScriptDialogsEnabled(&self, aredefaultscriptdialogsenabled: BOOL) -> HRESULT;
    pub unsafe fn GetIsStatusBarEnabled(&self, isstatusbarenabled: *mut BOOL) -> HRESULT;
    pub unsafe fn PutIsStatusBarEnabled(&self, isstatusbarenabled: BOOL) -> HRESULT;
    pub unsafe fn GetAreDevToolsEnabled(&self, aredevtoolsenabled: *mut BOOL) -> HRESULT;
    pub unsafe fn PutAreDevToolsEnabled(&self, aredevtoolsenabled: BOOL) -> HRESULT;
    pub unsafe fn GetAreDefaultContextMenusEnabled(&self, aredefaultcontextmenusenabled: *mut BOOL) -> HRESULT;
    pub unsafe fn PutAreDefaultContextMenusEnabled(&self, aredefaultcontextmenusenabled: BOOL) -> HRESULT;
    pub unsafe fn GetAreHostObjectsAllowed(&self, arehostobjectsallowed: *mut BOOL) -> HRESULT;
    pub unsafe fn PutAreHostObjectsAllowed(&self, arehostobjectsallowed: BOOL) -> HRESULT;
    pub unsafe fn GetIsZoomControlEnabled(&self, iszoomcontrolenabled: *mut BOOL) -> HRESULT;
    pub unsafe fn PutIsZoomControlEnabled(&self, iszoomcontrolenabled: BOOL) -> HRESULT;
    pub unsafe fn GetIsBuiltInErrorPageEnabled(&self, isbuiltinerrorpageenabled: *mut BOOL) -> HRESULT;
    pub unsafe fn PutIsBuiltInErrorPageEnabled(&self, isbuiltinerrorpageenabled: BOOL) -> HRESULT;
}

#[windows::core::interface("0fcf683a-93d1-4476-8440-9063622416b9")]
pub unsafe trait ICoreWebView2WebMessageReceivedEventArgs: IUnknown {
    pub unsafe fn GetSource(&self, source: *mut *mut u16) -> HRESULT;
    pub unsafe fn GetWebMessageAsJson(&self, webmessageasjson: *mut *mut u16) -> HRESULT;
    pub unsafe fn TryGetWebMessageAsString(&self, webmessageasstring: *mut *mut u16) -> HRESULT;
}

#[windows::core::interface("5b8649b9-4047-4d99-8bb6-01c277717d23")]
pub unsafe trait ICoreWebView2NavigationStartingEventArgs: IUnknown {
    pub unsafe fn GetUri(&self, uri: *mut *mut u16) -> HRESULT;
    pub unsafe fn GetIsUserInitiated(&self, isuserinitiated: *mut BOOL) -> HRESULT;
    pub unsafe fn GetIsRedirected(&self, isredirected: *mut BOOL) -> HRESULT;
    pub unsafe fn GetRequestHeaders(&self, requestheaders: *mut *mut c_void) -> HRESULT;
    pub unsafe fn GetCancel(&self, cancel: *mut BOOL) -> HRESULT;
    pub unsafe fn PutCancel(&self, cancel: BOOL) -> HRESULT;
    pub unsafe fn GetNavigationId(&self, navigationid: *mut u64) -> HRESULT;
}
