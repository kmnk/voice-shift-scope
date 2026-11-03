//! Writes the inside of the window to a PNG, used only in the end-to-end UI test.
//! On Windows it captures the window's client area with PrintWindow (PW_RENDERFULLCONTENT, which also renders the WebView2 content).
//! On other OSes it does not capture and returns the reason.

use std::path::Path;

#[cfg(windows)]
pub fn capture(window: &tauri::WebviewWindow, path: &Path) -> Result<(), String> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };
    use windows_sys::Win32::Storage::Xps::PrintWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect;

    // PW_CLIENTONLY | PW_RENDERFULLCONTENT
    const FLAGS: u32 = 0x1 | 0x2;

    let hwnd = window.hwnd().map_err(|e| e.to_string())?.0 as windows_sys::Win32::Foundation::HWND;
    let (width, height, mut pixels) = unsafe {
        let mut rect: RECT = std::mem::zeroed();
        if GetClientRect(hwnd, &mut rect) == 0 {
            return Err("GetClientRect failed".into());
        }
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        if w <= 0 || h <= 0 {
            return Err(format!("the window size is 0 ({w}×{h})"));
        }
        let screen_dc = GetDC(hwnd);
        let mem_dc = CreateCompatibleDC(screen_dc);
        let bitmap = CreateCompatibleBitmap(screen_dc, w, h);
        let old = SelectObject(mem_dc, bitmap as _);
        let printed = PrintWindow(hwnd, mem_dc, FLAGS);
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = w;
        info.bmiHeader.biHeight = -h; // top-down rows
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB as _;
        let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
        let lines = GetDIBits(mem_dc, bitmap, 0, h as u32, buf.as_mut_ptr() as _, &mut info, DIB_RGB_COLORS);
        SelectObject(mem_dc, old);
        DeleteObject(bitmap as _);
        DeleteDC(mem_dc);
        ReleaseDC(hwnd, screen_dc);
        if printed == 0 {
            return Err("PrintWindow failed".into());
        }
        if lines == 0 {
            return Err("GetDIBits failed".into());
        }
        (w as u32, h as u32, buf)
    };
    // Convert BGRA to RGBA and drop the alpha
    for px in pixels.chunks_exact_mut(4) {
        px.swap(0, 2);
        px[3] = 255;
    }
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(&pixels).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(not(windows))]
pub fn capture(_window: &tauri::WebviewWindow, _path: &Path) -> Result<(), String> {
    Err("window shots are taken only on Windows".into())
}
