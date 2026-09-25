//! Converts bounded, placeable Windows metafiles embedded by Word/MathType
//! into ordinary PNG resources. The original WMF never enters the managed
//! image store or the WebView.

const PLACEABLE_KEY: u32 = 0x9ac6_cdd7;
const MAX_WMF_BYTES: usize = 8 * 1024 * 1024;
const MAX_RENDER_DIMENSION: u32 = 4_096;
const MAX_RENDER_PIXELS: u64 = 4_000_000;

struct PlaceableHeader {
    left: i16,
    top: i16,
    right: i16,
    bottom: i16,
    inch: u16,
    checksum: u16,
    width: u32,
    height: u32,
}

fn word(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn dword(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
}

fn validate_placeable_wmf(bytes: &[u8]) -> Result<PlaceableHeader, String> {
    if bytes.len() < 46 || bytes.len() > MAX_WMF_BYTES || !bytes.len().is_multiple_of(2) {
        return Err("WMF 图片大小或结构无效".to_owned());
    }
    if dword(bytes, 0) != PLACEABLE_KEY || word(bytes, 4) != 0 || dword(bytes, 16) != 0 {
        return Err("WMF 图片头无效".to_owned());
    }
    let checksum = word(bytes, 20);
    let computed = (0..20)
        .step_by(2)
        .fold(0u16, |sum, offset| sum ^ word(bytes, offset));
    if checksum != computed {
        return Err("WMF 图片校验失败，文件可能已损坏".to_owned());
    }
    let left = word(bytes, 6) as i16;
    let top = word(bytes, 8) as i16;
    let right = word(bytes, 10) as i16;
    let bottom = word(bytes, 12) as i16;
    let inch = word(bytes, 14);
    let span_x = i32::from(right) - i32::from(left);
    let span_y = i32::from(bottom) - i32::from(top);
    if span_x <= 0 || span_y <= 0 || inch == 0 || inch > i16::MAX as u16 {
        return Err("WMF 图片的尺寸无效".to_owned());
    }
    // Placeable WMF bounds are logical units per inch. Render at screen DPI
    // so the image keeps approximately the same size in the question editor.
    let width = ((span_x as u64 * 96).div_ceil(u64::from(inch))).max(1) as u32;
    let height = ((span_y as u64 * 96).div_ceil(u64::from(inch))).max(1) as u32;
    if width > MAX_RENDER_DIMENSION
        || height > MAX_RENDER_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_RENDER_PIXELS
    {
        return Err("WMF 图片转换后的尺寸超过安全上限".to_owned());
    }

    let file_type = word(bytes, 22);
    let header_words = word(bytes, 24);
    let version = word(bytes, 26);
    let declared_words = dword(bytes, 28) as usize;
    if !matches!(file_type, 1 | 2)
        || header_words != 9
        || !matches!(version, 0x0100 | 0x0200 | 0x0300)
        || declared_words.checked_mul(2) != Some(bytes.len() - 22)
    {
        return Err("WMF 图片的内部文件头无效".to_owned());
    }
    let mut offset = 40usize;
    let mut found_end = false;
    while offset + 6 <= bytes.len() {
        let record_words = dword(bytes, offset) as usize;
        let record_bytes = record_words
            .checked_mul(2)
            .ok_or_else(|| "WMF 图片记录长度无效".to_owned())?;
        let end = offset
            .checked_add(record_bytes)
            .ok_or_else(|| "WMF 图片记录长度无效".to_owned())?;
        if record_words < 3 || end > bytes.len() {
            return Err("WMF 图片记录不完整".to_owned());
        }
        let operation = word(bytes, offset + 4);
        // META_ESCAPE can carry printer or application commands. The MathType
        // files use only MFCOMMENT (15), which is inert metadata.
        if operation == 0x0626 {
            if record_bytes < 10 || word(bytes, offset + 6) != 15 {
                return Err("WMF 图片含有不支持的外部命令".to_owned());
            }
            let comment_bytes = word(bytes, offset + 8) as usize;
            if comment_bytes > record_bytes - 10 {
                return Err("WMF 图片注释长度无效".to_owned());
            }
        }
        offset = end;
        if operation == 0 {
            found_end = record_words == 3 && offset == bytes.len();
            break;
        }
    }
    if !found_end {
        return Err("WMF 图片缺少完整的结束记录".to_owned());
    }
    Ok(PlaceableHeader {
        left,
        top,
        right,
        bottom,
        inch,
        checksum,
        width,
        height,
    })
}

pub(super) fn convert_placeable_wmf_to_png(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let header = validate_placeable_wmf(bytes)?;
    #[cfg(windows)]
    {
        render_windows(bytes, &header)
    }
    #[cfg(not(windows))]
    {
        let _ = header;
        Err("当前系统无法转换 WMF 图片，请在 Word 或 WPS 中将其替换为 PNG".to_owned())
    }
}

#[cfg(windows)]
fn render_windows(bytes: &[u8], header: &PlaceableHeader) -> Result<Vec<u8>, String> {
    use std::{ptr, slice};
    use windows_sys::Win32::Graphics::{Gdi, GdiPlus as Gp};

    const PIXEL_FORMAT_32BPP_ARGB: i32 = 0x0026_200a;

    struct Session(usize);
    impl Drop for Session {
        fn drop(&mut self) {
            unsafe { Gp::GdiplusShutdown(self.0) };
        }
    }
    struct MetaHandle(Gdi::HMETAFILE);
    impl Drop for MetaHandle {
        fn drop(&mut self) {
            unsafe { Gdi::DeleteMetaFile(self.0) };
        }
    }
    struct Image(*mut Gp::GpImage);
    impl Drop for Image {
        fn drop(&mut self) {
            unsafe { Gp::GdipDisposeImage(self.0) };
        }
    }
    struct Graphics(*mut Gp::GpGraphics);
    impl Drop for Graphics {
        fn drop(&mut self) {
            unsafe { Gp::GdipDeleteGraphics(self.0) };
        }
    }
    struct BitmapLock(*mut Gp::GpBitmap, Gp::BitmapData);
    impl Drop for BitmapLock {
        fn drop(&mut self) {
            unsafe { Gp::GdipBitmapUnlockBits(self.0, &raw mut self.1) };
        }
    }
    fn check(status: Gp::Status, step: &str) -> Result<(), String> {
        if status == 0 {
            Ok(())
        } else {
            Err(format!("Windows 图形转换在{step}时失败（错误码 {status}）"))
        }
    }

    let mut token = 0usize;
    let startup = Gp::GdiplusStartupInput {
        GdiplusVersion: 1,
        SuppressExternalCodecs: 1,
        ..Default::default()
    };
    check(
        unsafe { Gp::GdiplusStartup(&mut token, &startup, ptr::null_mut()) },
        "启动",
    )?;
    let _session = Session(token);
    let wmf_size = u32::try_from(bytes.len() - 22).map_err(|_| "WMF 文件过大".to_owned())?;
    let handle = unsafe { Gdi::SetMetaFileBitsEx(wmf_size, bytes[22..].as_ptr()) };
    if handle.is_null() {
        return Err("Windows 无法读取这张 WMF 图片".to_owned());
    }
    let _handle = MetaHandle(handle);
    let placeable = Gp::WmfPlaceableFileHeader {
        Key: PLACEABLE_KEY,
        Hmf: 0,
        BoundingBox: Gp::PWMFRect16 {
            Left: header.left,
            Top: header.top,
            Right: header.right,
            Bottom: header.bottom,
        },
        Inch: header.inch as i16,
        Reserved: 0,
        Checksum: header.checksum as i16,
    };
    let mut metafile = ptr::null_mut();
    check(
        unsafe { Gp::GdipCreateMetafileFromWmf(handle, 0, &placeable, &mut metafile) },
        "读取图形",
    )?;
    let metafile = Image(metafile.cast());

    let width = i32::try_from(header.width).map_err(|_| "WMF 宽度无效".to_owned())?;
    let height = i32::try_from(header.height).map_err(|_| "WMF 高度无效".to_owned())?;
    let mut bitmap = ptr::null_mut();
    check(
        unsafe {
            Gp::GdipCreateBitmapFromScan0(
                width,
                height,
                0,
                PIXEL_FORMAT_32BPP_ARGB,
                ptr::null(),
                &mut bitmap,
            )
        },
        "创建位图",
    )?;
    let bitmap = Image(bitmap.cast());
    let mut graphics = ptr::null_mut();
    check(
        unsafe { Gp::GdipGetImageGraphicsContext(bitmap.0, &mut graphics) },
        "准备绘制",
    )?;
    let graphics = Graphics(graphics);
    check(
        unsafe { Gp::GdipGraphicsClear(graphics.0, 0xffff_ffff) },
        "清理画布",
    )?;
    check(
        unsafe { Gp::GdipSetSmoothingMode(graphics.0, Gp::SmoothingModeAntiAlias) },
        "设置绘制质量",
    )?;
    check(
        unsafe { Gp::GdipDrawImageRectI(graphics.0, metafile.0, 0, 0, width, height) },
        "绘制图形",
    )?;
    drop(graphics);

    let rect = Gp::Rect {
        X: 0,
        Y: 0,
        Width: width,
        Height: height,
    };
    let mut bitmap_data = Gp::BitmapData::default();
    check(
        unsafe {
            Gp::GdipBitmapLockBits(
                bitmap.0.cast(),
                &rect,
                Gp::ImageLockModeRead as u32,
                PIXEL_FORMAT_32BPP_ARGB,
                &mut bitmap_data,
            )
        },
        "读取图像像素",
    )?;
    let locked = BitmapLock(bitmap.0.cast(), bitmap_data);
    let row_bytes = header.width as usize * 4;
    if locked.1.Scan0.is_null() || (locked.1.Stride.unsigned_abs() as usize) < row_bytes {
        return Err("Windows 返回的图像像素布局无效".to_owned());
    }
    let mut rgb = Vec::with_capacity(header.width as usize * header.height as usize * 3);
    for y in 0..header.height as isize {
        let row = unsafe {
            slice::from_raw_parts(
                locked
                    .1
                    .Scan0
                    .cast::<u8>()
                    .offset(y * locked.1.Stride as isize),
                row_bytes,
            )
        };
        for pixel in row.chunks_exact(4) {
            rgb.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
        }
    }
    drop(locked);

    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, header.width, header.height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|error| format!("WMF 转 PNG 时无法创建文件头：{error}"))?;
        writer
            .write_image_data(&rgb)
            .map_err(|error| format!("WMF 转 PNG 时无法保存像素：{error}"))?;
    }
    if output.len() > MAX_WMF_BYTES {
        return Err("WMF 转换后的 PNG 超过图片安全上限".to_owned());
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_placeable_wmf() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&PLACEABLE_KEY.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        for bound in [0i16, 0, 96, 96] {
            bytes.extend_from_slice(&bound.to_le_bytes());
        }
        bytes.extend_from_slice(&96u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        let checksum = (0..20)
            .step_by(2)
            .fold(0u16, |sum, offset| sum ^ word(&bytes, offset));
        bytes.extend_from_slice(&checksum.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&9u16.to_le_bytes());
        bytes.extend_from_slice(&0x0300u16.to_le_bytes());
        bytes.extend_from_slice(&12u32.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes
    }

    #[test]
    fn checks_bounds_checksum_and_records_before_rendering() {
        let source = empty_placeable_wmf();
        let header = validate_placeable_wmf(&source).unwrap();
        assert_eq!((header.width, header.height), (96, 96));

        let mut changed = source.clone();
        changed[20] ^= 1;
        assert!(validate_placeable_wmf(&changed).is_err());
        let mut changed = source.clone();
        changed[44] = 1;
        assert!(validate_placeable_wmf(&changed).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn renders_placeable_wmf_to_valid_png_without_external_programs() {
        let bytes = convert_placeable_wmf_to_png(&empty_placeable_wmf()).unwrap();
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    /// Opt-in visual regression for user-supplied WMFs kept outside the repo.
    #[cfg(windows)]
    #[test]
    #[ignore = "set ZHITIKU_WMF_FIXTURE and ZHITIKU_WMF_RENDER_OUTPUT"]
    fn renders_external_wmf_fixture() {
        let source = std::env::var_os("ZHITIKU_WMF_FIXTURE").expect("WMF fixture path required");
        let output = std::env::var_os("ZHITIKU_WMF_RENDER_OUTPUT").expect("PNG output required");
        let bytes = std::fs::read(source).expect("WMF fixture readable");
        let png = convert_placeable_wmf_to_png(&bytes).expect("WMF conversion succeeds");
        std::fs::write(output, png).expect("PNG output writable");
    }
}
