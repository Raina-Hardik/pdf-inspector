use crate::tables::{CellOccupancy, CellRect, Table, TableKind, TableSource};
use crate::types::{ItemType, PdfLine, PdfRect, TextItem};
use crate::{
    classify_pdf_mem, extract_pages_markdown_mem,
    extractor::{extract_text_with_positions_mem, group_into_lines_preserving_all_text},
    page_geometry_mem, process_pdf_mem_with_options, ImageInfo, LayoutComplexity, MarkdownProfile,
    PageGeometry, PageMarkdown, PageOcrReasons, PageRotation, PagesExtractionResult, PdfOptions,
    PdfProcessResult, PdfType, ProcessMode,
};
use serde::{Deserialize, Serialize};
use std::alloc::{alloc as rust_alloc, dealloc as rust_dealloc, Layout};
use std::slice;

#[no_mangle]
pub extern "C" fn alloc(size: usize) -> *mut u8 {
    if size == 0 {
        return std::ptr::null_mut();
    }
    let layout = match Layout::array::<u8>(size) {
        Ok(l) => l,
        Err(_) => return std::ptr::null_mut(),
    };
    unsafe { rust_alloc(layout) }
}

#[no_mangle]
pub extern "C" fn dealloc(ptr: *mut u8, size: usize) {
    if !ptr.is_null() && size > 0 {
        if let Ok(layout) = Layout::array::<u8>(size) {
            unsafe { rust_dealloc(ptr, layout) };
        }
    }
}

fn return_bytes(bytes: Vec<u8>) -> u64 {
    let len = bytes.len();
    if len == 0 {
        return 0;
    }
    let ptr = alloc(len);
    if ptr.is_null() {
        return 0;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, len);
    }
    ((ptr as u64) << 32) | (len as u64)
}

fn return_string(s: String) -> u64 {
    return_bytes(s.into_bytes())
}

fn return_error(err_msg: &str) -> u64 {
    let json = serde_json::json!({
        "error": err_msg
    });
    return_string(json.to_string())
}

fn catch_ffi<F>(f: F) -> u64
where
    F: FnOnce() -> u64 + std::panic::UnwindSafe,
{
    match std::panic::catch_unwind(f) {
        Ok(res) => res,
        Err(err) => {
            let msg = if let Some(s) = err.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = err.downcast_ref::<String>() {
                s.clone()
            } else {
                "Unknown Rust panic".to_string()
            };
            return_error(&format!("Rust panic: {msg}"))
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct FfiProcessOptions {
    pages: Option<Vec<u32>>,
    password: Option<String>,
    profile: Option<String>,
    include_page_markers: Option<bool>,
    include_images: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FfiPageOcrReasons {
    page: u32,
    reasons: Vec<String>,
}

impl From<PageOcrReasons> for FfiPageOcrReasons {
    fn from(v: PageOcrReasons) -> Self {
        Self {
            page: v.page,
            reasons: v.reasons,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FfiLayoutComplexity {
    is_complex: bool,
    pages_with_tables: Vec<u32>,
    pages_with_columns: Vec<u32>,
}

impl From<LayoutComplexity> for FfiLayoutComplexity {
    fn from(v: LayoutComplexity) -> Self {
        Self {
            is_complex: v.is_complex,
            pages_with_tables: v.pages_with_tables,
            pages_with_columns: v.pages_with_columns,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FfiPdfProcessResult {
    pdf_type: &'static str,
    markdown: Option<String>,
    page_count: u32,
    processing_time_ms: u64,
    pages_needing_ocr: Vec<u32>,
    ocr_reasons_by_page: Vec<FfiPageOcrReasons>,
    title: Option<String>,
    confidence: f32,
    layout: FfiLayoutComplexity,
    has_encoding_issues: bool,
}

impl From<PdfProcessResult> for FfiPdfProcessResult {
    fn from(v: PdfProcessResult) -> Self {
        let pdf_type = match v.pdf_type {
            PdfType::TextBased => "TextBased",
            PdfType::Scanned => "Scanned",
            PdfType::ImageBased => "ImageBased",
            PdfType::Mixed => "Mixed",
        };
        Self {
            pdf_type,
            markdown: v.markdown,
            page_count: v.page_count,
            processing_time_ms: v.processing_time_ms,
            pages_needing_ocr: v.pages_needing_ocr,
            ocr_reasons_by_page: v.ocr_reasons_by_page.into_iter().map(Into::into).collect(),
            title: v.title,
            confidence: v.confidence,
            layout: v.layout.into(),
            has_encoding_issues: v.has_encoding_issues,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FfiPdfClassification {
    pdf_type: &'static str,
    page_count: u32,
    pages_needing_ocr: Vec<u32>,
    confidence: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FfiPageMarkdown {
    page: u32,
    markdown: String,
    needs_ocr: bool,
    ocr_reason: Option<String>,
}

impl From<PageMarkdown> for FfiPageMarkdown {
    fn from(v: PageMarkdown) -> Self {
        Self {
            page: v.page + 1,
            markdown: v.markdown,
            needs_ocr: v.needs_ocr,
            ocr_reason: v.ocr_reason,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FfiPagesExtractionResult {
    pages: Vec<FfiPageMarkdown>,
    pages_with_tables: Vec<u32>,
    pages_with_columns: Vec<u32>,
    pages_needing_ocr: Vec<u32>,
    ocr_reasons_by_page: Vec<FfiPageOcrReasons>,
    is_complex: bool,
}

impl From<PagesExtractionResult> for FfiPagesExtractionResult {
    fn from(v: PagesExtractionResult) -> Self {
        Self {
            pages: v.pages.into_iter().map(Into::into).collect(),
            pages_with_tables: v.pages_with_tables,
            pages_with_columns: v.pages_with_columns,
            pages_needing_ocr: v.pages_needing_ocr,
            ocr_reasons_by_page: v.ocr_reasons_by_page.into_iter().map(Into::into).collect(),
            is_complex: v.is_complex,
        }
    }
}

#[no_mangle]
pub extern "C" fn ffi_process_pdf(
    pdf_ptr: *const u8,
    pdf_len: usize,
    opts_ptr: *const u8,
    opts_len: usize,
) -> u64 {
    catch_ffi(|| {
        if pdf_ptr.is_null() || pdf_len == 0 {
            return return_error("Empty or null PDF buffer");
        }
        let pdf_bytes = unsafe { slice::from_raw_parts(pdf_ptr, pdf_len) };

        let mut opts = PdfOptions::new().mode(ProcessMode::Full);
        if !opts_ptr.is_null() && opts_len > 0 {
            let opts_slice = unsafe { slice::from_raw_parts(opts_ptr, opts_len) };
            if let Ok(parsed) = serde_json::from_slice::<FfiProcessOptions>(opts_slice) {
                if let Some(pages) = parsed.pages {
                    if pages.contains(&0) {
                        return return_error("Invalid page index: pages must be 1-indexed (>= 1)");
                    }
                    opts = opts.pages(pages);
                }
                if let Some(password) = parsed.password {
                    opts = opts.password(password);
                }
                if let Some(profile) = parsed.profile {
                    match profile.as_str() {
                        "compact" => opts.markdown.profile = MarkdownProfile::Compact,
                        "fidelity" => opts.markdown.profile = MarkdownProfile::Fidelity,
                        _ => {
                            return return_error(
                                "Invalid markdown profile: expected 'fidelity' or 'compact'",
                            )
                        }
                    }
                }
                if let Some(inc) = parsed.include_page_markers {
                    opts.markdown.include_page_numbers = inc;
                }
                if let Some(inc) = parsed.include_images {
                    opts.markdown.include_images = inc;
                }
            }
        }

        let start_time = std::time::Instant::now();
        match process_pdf_mem_with_options(pdf_bytes, opts) {
            Ok(mut res) => {
                res.processing_time_ms = start_time.elapsed().as_millis() as u64;
                let ffi_res = FfiPdfProcessResult::from(res);
                match serde_json::to_string(&ffi_res) {
                    Ok(json) => return_string(json),
                    Err(e) => return_error(&format!("Serialization error: {e}")),
                }
            }
            Err(e) => return_error(&format!("PDF processing error: {e}")),
        }
    })
}

#[no_mangle]
pub extern "C" fn ffi_detect_pdf(
    pdf_ptr: *const u8,
    pdf_len: usize,
    password_ptr: *const u8,
    password_len: usize,
) -> u64 {
    catch_ffi(|| {
        if pdf_ptr.is_null() || pdf_len == 0 {
            return return_error("Empty or null PDF buffer");
        }
        let pdf_bytes = unsafe { slice::from_raw_parts(pdf_ptr, pdf_len) };

        let mut opts = PdfOptions::detect_only();
        if !password_ptr.is_null() && password_len > 0 {
            let pwd_slice = unsafe { slice::from_raw_parts(password_ptr, password_len) };
            if let Ok(pwd) = std::str::from_utf8(pwd_slice) {
                opts = opts.password(pwd);
            }
        }

        let start_time = std::time::Instant::now();
        match process_pdf_mem_with_options(pdf_bytes, opts) {
            Ok(mut res) => {
                res.processing_time_ms = start_time.elapsed().as_millis() as u64;
                let ffi_res = FfiPdfProcessResult::from(res);
                match serde_json::to_string(&ffi_res) {
                    Ok(json) => return_string(json),
                    Err(e) => return_error(&format!("Serialization error: {e}")),
                }
            }
            Err(e) => return_error(&format!("PDF detection error: {e}")),
        }
    })
}

#[no_mangle]
pub extern "C" fn ffi_classify_pdf(pdf_ptr: *const u8, pdf_len: usize) -> u64 {
    catch_ffi(|| {
        if pdf_ptr.is_null() || pdf_len == 0 {
            return return_error("Empty or null PDF buffer");
        }
        let pdf_bytes = unsafe { slice::from_raw_parts(pdf_ptr, pdf_len) };

        match classify_pdf_mem(pdf_bytes) {
            Ok(res) => {
                let pdf_type = match res.pdf_type {
                    PdfType::TextBased => "TextBased",
                    PdfType::Scanned => "Scanned",
                    PdfType::ImageBased => "ImageBased",
                    PdfType::Mixed => "Mixed",
                };
                let ffi_res = FfiPdfClassification {
                    pdf_type,
                    page_count: res.page_count,
                    pages_needing_ocr: res.pages_needing_ocr,
                    confidence: res.confidence,
                };
                match serde_json::to_string(&ffi_res) {
                    Ok(json) => return_string(json),
                    Err(e) => return_error(&format!("Serialization error: {e}")),
                }
            }
            Err(e) => return_error(&format!("PDF classification error: {e}")),
        }
    })
}

#[no_mangle]
pub extern "C" fn ffi_extract_text(pdf_ptr: *const u8, pdf_len: usize) -> u64 {
    catch_ffi(|| {
        if pdf_ptr.is_null() || pdf_len == 0 {
            return return_error("Empty or null PDF buffer");
        }
        let pdf_bytes = unsafe { slice::from_raw_parts(pdf_ptr, pdf_len) };

        match extract_text_with_positions_mem(pdf_bytes) {
            Ok(items) => {
                let lines = group_into_lines_preserving_all_text(items);
                let text = lines
                    .into_iter()
                    .map(|line| line.text())
                    .filter(|line| !line.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");
                let json = serde_json::json!({ "text": text });
                return_string(json.to_string())
            }
            Err(e) => return_error(&format!("Text extraction error: {e}")),
        }
    })
}

#[no_mangle]
pub extern "C" fn ffi_extract_pages_markdown(
    pdf_ptr: *const u8,
    pdf_len: usize,
    pages_ptr: *const u8,
    pages_len: usize,
) -> u64 {
    catch_ffi(|| {
        if pdf_ptr.is_null() || pdf_len == 0 {
            return return_error("Empty or null PDF buffer");
        }
        let pdf_bytes = unsafe { slice::from_raw_parts(pdf_ptr, pdf_len) };

        let pages: Option<Vec<u32>> = if !pages_ptr.is_null() && pages_len > 0 {
            let slice = unsafe { slice::from_raw_parts(pages_ptr, pages_len) };
            if let Ok(parsed) = serde_json::from_slice::<Vec<u32>>(slice) {
                if parsed.contains(&0) {
                    return return_error("Invalid page index: pages must be 1-indexed (>= 1)");
                }
                Some(parsed.into_iter().map(|p| p - 1).collect())
            } else {
                None
            }
        } else {
            None
        };

        match extract_pages_markdown_mem(pdf_bytes, pages.as_deref()) {
            Ok(res) => {
                let ffi_res = FfiPagesExtractionResult::from(res);
                match serde_json::to_string(&ffi_res) {
                    Ok(json) => return_string(json),
                    Err(e) => return_error(&format!("Serialization error: {e}")),
                }
            }
            Err(e) => return_error(&format!("Pages markdown extraction error: {e}")),
        }
    })
}

// ---------------------------------------------------------------------------
// ffi_page_geometry
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FfiTextItem {
    text: String,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    font: String,
    font_size: f32,
    page: u32,
    is_bold: bool,
    is_italic: bool,
    is_underline: bool,
    is_strikeout: bool,
    item_type: &'static str,
    /// The target URL for an `ItemType::Link(url)` item; `None` for every
    /// other item type. `itemType` alone said only "Link" and dropped the
    /// URL the enum variant was already carrying.
    #[serde(skip_serializing_if = "Option::is_none")]
    link_url: Option<String>,
    mcid: Option<i64>,
    /// The item's baseline angle in degrees, in the same frame as its
    /// coordinates (see `FfiPageGeometry::rotation`). 0 for horizontal text
    /// and for non-text items, which carry no text matrix.
    rotation: f32,
}

impl From<&TextItem> for FfiTextItem {
    fn from(v: &TextItem) -> Self {
        let item_type = match v.item_type {
            ItemType::Text => "Text",
            ItemType::Image => "Image",
            ItemType::Link(_) => "Link",
            ItemType::FormField => "FormField",
        };
        let link_url = match &v.item_type {
            ItemType::Link(url) => Some(url.clone()),
            _ => None,
        };
        Self {
            text: v.text.clone(),
            x: v.x,
            y: v.y,
            width: v.width,
            height: v.height,
            font: v.font.clone(),
            font_size: v.font_size,
            page: v.page,
            is_bold: v.is_bold,
            is_italic: v.is_italic,
            is_underline: v.is_underline,
            is_strikeout: v.is_strikeout,
            item_type,
            link_url,
            mcid: v.mcid,
            rotation: v.rotation,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FfiPdfLine {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    page: u32,
}

impl From<&PdfLine> for FfiPdfLine {
    fn from(v: &PdfLine) -> Self {
        Self {
            x1: v.x1,
            y1: v.y1,
            x2: v.x2,
            y2: v.y2,
            page: v.page,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FfiPdfRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    page: u32,
}

impl From<&PdfRect> for FfiPdfRect {
    fn from(v: &PdfRect) -> Self {
        Self {
            x: v.x,
            y: v.y,
            width: v.width,
            height: v.height,
            page: v.page,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FfiImageInfo {
    xobject_name: String,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    page: u32,
}

impl From<&ImageInfo> for FfiImageInfo {
    fn from(v: &ImageInfo) -> Self {
        Self {
            xobject_name: v.xobject_name.clone(),
            x: v.x,
            y: v.y,
            width: v.width,
            height: v.height,
            page: v.page,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FfiCellRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl From<&CellRect> for FfiCellRect {
    fn from(v: &CellRect) -> Self {
        Self {
            x: v.x,
            y: v.y,
            width: v.width,
            height: v.height,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FfiCellOccupancy {
    is_own: bool,
    rect: Option<FfiCellRect>,
}

impl From<&CellOccupancy> for FfiCellOccupancy {
    fn from(v: &CellOccupancy) -> Self {
        Self {
            is_own: v.is_own,
            rect: v.rect.as_ref().map(FfiCellRect::from),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FfiTable {
    columns: Vec<f32>,
    rows: Vec<f32>,
    cells: Vec<Vec<String>>,
    kind: &'static str,
    source: &'static str,
    /// `None` when the detector that produced this table has no per-cell
    /// rect evidence to offer (see `Table::cell_occupancy`) — not "all cells
    /// empty".
    cell_occupancy: Option<Vec<Vec<FfiCellOccupancy>>>,
}

impl From<&Table> for FfiTable {
    fn from(v: &Table) -> Self {
        let kind = match v.kind {
            TableKind::Data => "Data",
            TableKind::Toc => "Toc",
        };
        let source = match v.source {
            TableSource::Unspecified => "Unspecified",
            TableSource::Lines => "Lines",
            TableSource::Rects => "Rects",
            TableSource::Struct => "Struct",
            TableSource::Heuristic => "Heuristic",
        };
        Self {
            columns: v.columns.clone(),
            rows: v.rows.clone(),
            cells: v.cells.clone(),
            kind,
            source,
            cell_occupancy: v.cell_occupancy.as_ref().map(|rows| {
                rows.iter()
                    .map(|row| row.iter().map(FfiCellOccupancy::from).collect())
                    .collect()
            }),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FfiPageGeometry {
    page: u32,
    text_items: Vec<FfiTextItem>,
    lines: Vec<FfiPdfLine>,
    rects: Vec<FfiPdfRect>,
    images: Vec<FfiImageInfo>,
    tables: Vec<FfiTable>,
    /// How this page's frame was turned so predominantly rotated text reads
    /// left-to-right: "Upright", "Ccw" or "Cw" (see `PageGeometry::rotation`).
    /// Every coordinate and every item `rotation` on this page is ALREADY in
    /// the turned frame — a consumer must not apply this a second time.
    rotation: &'static str,
    /// `[x0, y0, x1, y1]` of the visible page box (`CropBox ∩ MediaBox`) in
    /// raw PDF user space, the box these coordinates are relative to. Absent
    /// when the page declares no usable box.
    #[serde(skip_serializing_if = "Option::is_none")]
    page_box: Option<[f32; 4]>,
}

impl From<&PageGeometry> for FfiPageGeometry {
    fn from(v: &PageGeometry) -> Self {
        Self {
            page: v.page,
            text_items: v.text_items.iter().map(FfiTextItem::from).collect(),
            lines: v.lines.iter().map(FfiPdfLine::from).collect(),
            rects: v.rects.iter().map(FfiPdfRect::from).collect(),
            images: v.images.iter().map(FfiImageInfo::from).collect(),
            tables: v.tables.iter().map(FfiTable::from).collect(),
            rotation: match v.rotation {
                PageRotation::Upright => "Upright",
                PageRotation::Ccw => "Ccw",
                PageRotation::Cw => "Cw",
            },
            page_box: v.page_box.map(|(x0, y0, x1, y1)| [x0, y0, x1, y1]),
        }
    }
}

/// Per-page raw layout geometry: text items (with rotation), line segments,
/// rects, image placeholders, and detected tables (with per-cell occupancy
/// where the detector has real evidence — see `Table::cell_occupancy`).
/// Additive: does not touch `ffi_process_pdf`'s markdown pipeline or output.
#[no_mangle]
pub extern "C" fn ffi_page_geometry(pdf_ptr: *const u8, pdf_len: usize) -> u64 {
    catch_ffi(|| {
        if pdf_ptr.is_null() || pdf_len == 0 {
            return return_error("Empty or null PDF buffer");
        }
        let pdf_bytes = unsafe { slice::from_raw_parts(pdf_ptr, pdf_len) };

        match page_geometry_mem(pdf_bytes) {
            Ok(pages) => {
                let ffi_pages: Vec<FfiPageGeometry> =
                    pages.iter().map(FfiPageGeometry::from).collect();
                match serde_json::to_string(&ffi_pages) {
                    Ok(json) => return_string(json),
                    Err(e) => return_error(&format!("Serialization error: {e}")),
                }
            }
            Err(e) => return_error(&format!("Page geometry extraction error: {e}")),
        }
    })
}

#[no_mangle]
pub extern "C" fn ffi_version() -> u64 {
    catch_ffi(|| {
        let json = serde_json::json!({ "version": env!("CARGO_PKG_VERSION") });
        return_string(json.to_string())
    })
}
