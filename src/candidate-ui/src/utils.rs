use tao::window::Window;
use windows::Win32::{
    Foundation::RECT,
    Graphics::Gdi::{GetMonitorInfoW, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONEAREST},
};

pub fn candidate_position(
    top: i32,
    left: i32,
    bottom: i32,
    right: i32,
    window: &Window,
) -> (f64, f64) {
    let mut x = left - 15;
    let mut y = bottom;

    let monitor = unsafe {
        MonitorFromRect(
            &RECT {
                left,
                top,
                right,
                bottom,
            },
            MONITOR_DEFAULTTONEAREST,
        )
    };

    let mut info = MONITORINFO::default();
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    unsafe {
        let _ = GetMonitorInfoW(monitor, &mut info);
    }

    let size = window.inner_size();
    if y + size.height as i32 > info.rcWork.bottom {
        y = top - size.height as i32;
    }
    if x + size.width as i32 > info.rcWork.right {
        x = info.rcWork.right - size.width as i32;
    }
    if x < info.rcWork.left {
        x = info.rcWork.left;
    }

    (x as f64, y as f64)
}
