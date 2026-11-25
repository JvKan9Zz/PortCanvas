use crate::workbench_ui::Workbench;
use slint::winit_030::{
    winit::{
        raw_window_handle::{HasWindowHandle, RawWindowHandle},
        window::Window,
    },
    WinitWindowAccessor,
};
use slint::{ComponentHandle, Weak};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::{
        Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND},
        Gdi::{
            GetMonitorInfoW, MonitorFromRect, MonitorFromWindow, ScreenToClient, HMONITOR,
            MONITORINFO, MONITOR_DEFAULTTONEAREST,
        },
    },
    UI::{
        HiDpi::GetDpiForWindow,
        Shell::{
            DefSubclassProc, RemoveWindowSubclass, SHAppBarMessage, SetWindowSubclass, ABE_BOTTOM,
            ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_GETAUTOHIDEBAREX, APPBARDATA,
        },
        WindowsAndMessaging::*,
    },
};

const SUBCLASS_ID: usize = 1;

pub fn install(ui: &Workbench) -> Result<(), slint::EventLoopError> {
    let weak = ui.as_weak();
    ui.on_window_minimize({
        let weak = weak.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                ui.window().set_minimized(true);
            }
        }
    });
    ui.on_window_toggle_maximize({
        let weak = weak.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                let window = ui.window();
                if window.is_fullscreen() {
                    window.set_fullscreen(false);
                    ui.set_window_is_fullscreen(false);
                } else {
                    window.with_winit_window(|window| {
                        if let Some(hwnd) = hwnd(window) {
                            unsafe {
                                let command = if IsZoomed(hwnd).as_bool() {
                                    SW_RESTORE
                                } else {
                                    SW_MAXIMIZE
                                };
                                let _ = ShowWindow(hwnd, command);
                            }
                        }
                    });
                }
                sync_maximized(&ui);
            }
        }
    });
    ui.on_window_close({
        let weak = weak.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                ui.window().with_winit_window(|window| {
                    if let Some(hwnd) = hwnd(window) {
                        // Use the same close request and teardown as Alt+F4.
                        let _ = unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
                    }
                });
            }
        }
    });
    slint::spawn_local(async move {
        let Some(ui) = weak.upgrade() else { return };
        let Ok(window) = ui.window().winit_window().await else {
            return;
        };
        if let Some(hwnd) = hwnd(&window) {
            ui.set_custom_caption(unsafe { install_frame(hwnd, weak) });
        }
    })?;
    Ok(())
}

pub fn sync_maximized(ui: &Workbench) {
    let _ = ui.window().with_winit_window(|window| {
        if let Some(hwnd) = hwnd(window) {
            unsafe {
                if !IsIconic(hwnd).as_bool() {
                    let maximized = IsZoomed(hwnd).as_bool();
                    ui.set_window_is_maximized(maximized);
                    if maximized && ui.get_custom_caption() && framed(hwnd) {
                        // winit can restore an overscanned outer rectangle
                        // after leaving fullscreen. Recheck after its event.
                        let _ = ui.as_weak().upgrade_in_event_loop(|ui| {
                            ui.window().with_winit_window(|window| {
                                if let Some(hwnd) = self::hwnd(window) {
                                    fit_maximized_window(hwnd);
                                }
                            });
                        });
                    }
                }
            }
        }
    });
}

fn hwnd(window: &Window) -> Option<HWND> {
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(HWND(handle.hwnd.get() as *mut _)),
        _ => None,
    }
}

unsafe fn install_frame(hwnd: HWND, ui: Weak<Workbench>) -> bool {
    let data = Box::into_raw(Box::new(ui));
    unsafe {
        if !SetWindowSubclass(hwnd, Some(frame_proc), SUBCLASS_ID, data as usize).as_bool() {
            drop(Box::from_raw(data));
            return false;
        }
        // Recalculate the frame that was created by winit. The subclass owns
        // data on this UI thread until WM_NCDESTROY.
        if SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        )
        .is_err()
        {
            if RemoveWindowSubclass(hwnd, Some(frame_proc), SUBCLASS_ID).as_bool() {
                drop(Box::from_raw(data));
            }
            return false;
        }
        let corners = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            std::ptr::from_ref(&corners).cast(),
            std::mem::size_of_val(&corners) as u32,
        );
    }
    true
}

unsafe extern "system" fn frame_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    data: usize,
) -> LRESULT {
    unsafe {
        match message {
            WM_GETMINMAXINFO if framed(hwnd) => {
                // Let winit set its minimum tracking dimensions. Match the
                // outer maximized window to the same work area as its contents
                // so the invisible frame cannot intercept the taskbar edge.
                let result = DefSubclassProc(hwnd, message, wparam, lparam);
                let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                if let Some(info) = monitor_info(monitor) {
                    let bounds = maximized_client_rect(&info);
                    let limits = &mut *(lparam.0 as *mut MINMAXINFO);
                    limits.ptMaxPosition = POINT {
                        x: bounds.left - info.rcMonitor.left,
                        y: bounds.top - info.rcMonitor.top,
                    };
                    limits.ptMaxSize = POINT {
                        x: bounds.right - bounds.left,
                        y: bounds.bottom - bounds.top,
                    };
                }
                return result;
            }
            WM_NCCALCSIZE if framed(hwnd) => {
                // With wParam set, the first field of NCCALCSIZE_PARAMS is
                // the proposed client RECT. Otherwise lParam is the RECT.
                let client = &mut *(lparam.0 as *mut RECT);
                if IsZoomed(hwnd).as_bool() {
                    let monitor = MonitorFromRect(client, MONITOR_DEFAULTTONEAREST);
                    if let Some(info) = monitor_info(monitor) {
                        *client = maximized_client_rect(&info);
                    }
                }
                return LRESULT(0);
            }
            WM_NCHITTEST if framed(hwnd) => {
                let mut point = POINT {
                    x: lparam.0 as i16 as i32,
                    y: (lparam.0 >> 16) as i16 as i32,
                };
                let dpi = GetDpiForWindow(hwnd).max(96);
                if !IsZoomed(hwnd).as_bool() {
                    let mut bounds = RECT::default();
                    if GetWindowRect(hwnd, &mut bounds).is_ok() {
                        let border = (6 * dpi as i32 + 48) / 96;
                        if let Some(edge) = resize_edge(bounds, point, border) {
                            return LRESULT(edge as isize);
                        }
                    }
                }
                if ScreenToClient(hwnd, &mut point).as_bool() {
                    let ui = &*(data as *const Weak<Workbench>);
                    if let Some(ui) = ui.upgrade() {
                        let scale = ui.window().scale_factor();
                        let x = point.x as f32 / scale;
                        let y = point.y as f32 / scale;
                        let drag_regions =
                            [ui.get_window_drag_region(), ui.get_brand_drag_region()];
                        if drag_regions.iter().any(|region| {
                            x >= region.x
                                && y >= region.y
                                && x < region.x + region.width
                                && y < region.y + region.height
                        }) {
                            return LRESULT(HTCAPTION as isize);
                        }
                    }
                }
                return LRESULT(HTCLIENT as isize);
            }
            WM_NCDESTROY => {
                let _ = RemoveWindowSubclass(hwnd, Some(frame_proc), id);
                drop(Box::from_raw(data as *mut Weak<Workbench>));
            }
            _ => {}
        }
        DefSubclassProc(hwnd, message, wparam, lparam)
    }
}

fn monitor_info(monitor: HMONITOR) -> Option<MONITORINFO> {
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        GetMonitorInfoW(monitor, &mut info)
            .as_bool()
            .then_some(info)
    }
}

fn fit_maximized_window(hwnd: HWND) {
    unsafe {
        if !IsZoomed(hwnd).as_bool() || !framed(hwnd) {
            return;
        }
        let Some(info) = monitor_info(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST)) else {
            return;
        };
        let target = maximized_client_rect(&info);
        let mut actual = RECT::default();
        if GetWindowRect(hwnd, &mut actual).is_ok() && actual != target {
            let _ = SetWindowPos(
                hwnd,
                None,
                target.left,
                target.top,
                target.right - target.left,
                target.bottom - target.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
}

fn maximized_client_rect(info: &MONITORINFO) -> RECT {
    let mut client = info.rcWork;
    // A hidden taskbar contributes no inset to rcWork. Leave a narrow strip
    // at its monitor edge so the shell can reveal it on pointer hover.
    for (edge, bound, reveal) in [
        (ABE_LEFT, &mut client.left, info.rcMonitor.left + 2),
        (ABE_TOP, &mut client.top, info.rcMonitor.top + 2),
        (ABE_RIGHT, &mut client.right, info.rcMonitor.right - 2),
        (ABE_BOTTOM, &mut client.bottom, info.rcMonitor.bottom - 2),
    ] {
        let mut bar = APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            uEdge: edge,
            rc: info.rcMonitor,
            ..Default::default()
        };
        if unsafe { SHAppBarMessage(ABM_GETAUTOHIDEBAREX, &mut bar) } != 0 {
            *bound = if edge == ABE_LEFT || edge == ABE_TOP {
                (*bound).max(reveal)
            } else {
                (*bound).min(reveal)
            };
        }
    }
    client
}

unsafe fn framed(hwnd: HWND) -> bool {
    // winit removes WS_CAPTION in true fullscreen. Its own fullscreen frame
    // handling must still run even if the last normal state was maximized.
    unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & WS_CAPTION.0 == WS_CAPTION.0 }
}

fn resize_edge(bounds: RECT, point: POINT, border: i32) -> Option<u32> {
    if point.x < bounds.left
        || point.x >= bounds.right
        || point.y < bounds.top
        || point.y >= bounds.bottom
    {
        return None;
    }
    let left = point.x < bounds.left + border;
    let right = point.x >= bounds.right - border;
    let top = point.y < bounds.top + border;
    let bottom = point.y >= bounds.bottom - border;
    match (left, right, top, bottom) {
        (true, _, true, _) => Some(HTTOPLEFT),
        (_, true, true, _) => Some(HTTOPRIGHT),
        (true, _, _, true) => Some(HTBOTTOMLEFT),
        (_, true, _, true) => Some(HTBOTTOMRIGHT),
        (true, _, _, _) => Some(HTLEFT),
        (_, true, _, _) => Some(HTRIGHT),
        (_, _, true, _) => Some(HTTOP),
        (_, _, _, true) => Some(HTBOTTOM),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_edges_keep_the_header_and_points_outside_the_frame_clear() {
        let bounds = RECT {
            left: -1200,
            top: 40,
            right: 0,
            bottom: 860,
        };
        for (x, y, expected) in [
            (-1200, 40, Some(HTTOPLEFT)),
            (-1, 40, Some(HTTOPRIGHT)),
            (-1200, 859, Some(HTBOTTOMLEFT)),
            (-1, 859, Some(HTBOTTOMRIGHT)),
            (-1200, 450, Some(HTLEFT)),
            (-1, 450, Some(HTRIGHT)),
            (-600, 48, Some(HTTOP)),
            (-600, 858, Some(HTBOTTOM)),
            (-600, 58, None),
            (0, 450, None),
        ] {
            assert_eq!(resize_edge(bounds, POINT { x, y }, 9), expected);
        }
    }
}
