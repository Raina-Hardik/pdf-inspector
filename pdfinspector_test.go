package pdfinspector_test

import (
	"os"
	"strings"
	"testing"

	pdfinspector "github.com/firecrawl/pdf-inspector"
)

func TestVersion(t *testing.T) {
	ver, err := pdfinspector.Version()
	if err != nil {
		t.Fatalf("Version failed: %v", err)
	}
	if ver == "" {
		t.Fatal("Version returned empty string")
	}
	t.Logf("pdf-inspector WASM version: %s", ver)
}

func TestProcessPdf(t *testing.T) {
	pdfData, err := os.ReadFile("tests/fixtures/thermo-freon12.pdf")
	if err != nil {
		t.Fatalf("Failed to read fixture: %v", err)
	}

	result, err := pdfinspector.ProcessPdf(pdfData, nil)
	if err != nil {
		t.Fatalf("ProcessPdf failed: %v", err)
	}

	if result.PdfType != "TextBased" {
		t.Errorf("Expected PdfType TextBased, got %s", result.PdfType)
	}
	if result.PageCount == 0 {
		t.Errorf("Expected PageCount > 0")
	}
	if result.Markdown == nil || len(*result.Markdown) == 0 {
		t.Errorf("Expected non-empty Markdown output")
	}

	t.Logf("PdfType: %s, PageCount: %d, Markdown len: %d", result.PdfType, result.PageCount, len(*result.Markdown))
}

func TestDetectPdf(t *testing.T) {
	pdfData, err := os.ReadFile("tests/fixtures/thermo-freon12.pdf")
	if err != nil {
		t.Fatalf("Failed to read fixture: %v", err)
	}

	result, err := pdfinspector.DetectPdf(pdfData, "")
	if err != nil {
		t.Fatalf("DetectPdf failed: %v", err)
	}

	if result.PdfType != "TextBased" {
		t.Errorf("Expected PdfType TextBased, got %s", result.PdfType)
	}
	if result.Markdown != nil {
		t.Errorf("Expected nil Markdown in detect-only mode")
	}
}

func TestClassifyPdf(t *testing.T) {
	pdfData, err := os.ReadFile("tests/fixtures/thermo-freon12.pdf")
	if err != nil {
		t.Fatalf("Failed to read fixture: %v", err)
	}

	classification, err := pdfinspector.ClassifyPdf(pdfData)
	if err != nil {
		t.Fatalf("ClassifyPdf failed: %v", err)
	}

	if classification.PdfType != "TextBased" {
		t.Errorf("Expected PdfType TextBased, got %s", classification.PdfType)
	}
	if classification.PageCount == 0 {
		t.Errorf("Expected PageCount > 0")
	}
}

func TestExtractText(t *testing.T) {
	pdfData, err := os.ReadFile("tests/fixtures/thermo-freon12.pdf")
	if err != nil {
		t.Fatalf("Failed to read fixture: %v", err)
	}

	text, err := pdfinspector.ExtractText(pdfData)
	if err != nil {
		t.Fatalf("ExtractText failed: %v", err)
	}

	if len(strings.TrimSpace(text)) == 0 {
		t.Fatalf("ExtractText returned empty string")
	}
	runes := []rune(text)
	if len(runes) > 100 {
		runes = runes[:100]
	}
	t.Logf("Extracted text snippet: %s", string(runes))
}

func TestEncryptedPdfWithPassword(t *testing.T) {
	pdfData, err := os.ReadFile("tests/fixtures/encrypted-secret123.pdf")
	if err != nil {
		t.Fatalf("Failed to read fixture: %v", err)
	}

	// Should fail without password
	_, err = pdfinspector.ProcessPdf(pdfData, nil)
	if err == nil {
		t.Fatal("Expected error for encrypted PDF without password, but got none")
	}

	// Should succeed with password
	opts := &pdfinspector.ProcessOptions{
		Password: "secret123",
	}
	res, err := pdfinspector.ProcessPdf(pdfData, opts)
	if err != nil {
		t.Fatalf("ProcessPdf with password failed: %v", err)
	}
	if res.Markdown == nil || len(*res.Markdown) == 0 {
		t.Fatal("Expected markdown from encrypted PDF with password")
	}
}

func TestExtractPagesMarkdown(t *testing.T) {
	pdfData, err := os.ReadFile("tests/fixtures/thermo-freon12.pdf")
	if err != nil {
		t.Fatalf("Failed to read fixture: %v", err)
	}

	res, err := pdfinspector.ExtractPagesMarkdown(pdfData, nil)
	if err != nil {
		t.Fatalf("ExtractPagesMarkdown failed: %v", err)
	}
	if len(res.Pages) == 0 {
		t.Fatal("Expected pages in result")
	}
	t.Logf("Extracted %d pages markdown", len(res.Pages))
}

func TestValidationAndMetrics(t *testing.T) {
	pdfData, err := os.ReadFile("tests/fixtures/thermo-freon12.pdf")
	if err != nil {
		t.Fatalf("Failed to read fixture: %v", err)
	}

	// 1. ProcessingTimeMs check (logged rather than strictly > 0 to avoid test flakiness on fast sub-ms runs)
	res, err := pdfinspector.ProcessPdf(pdfData, nil)
	if err != nil {
		t.Fatalf("ProcessPdf failed: %v", err)
	}
	t.Logf("ProcessPdf ProcessingTimeMs: %d", res.ProcessingTimeMs)

	detRes, err := pdfinspector.DetectPdf(pdfData, "")
	if err != nil {
		t.Fatalf("DetectPdf failed: %v", err)
	}
	t.Logf("DetectPdf ProcessingTimeMs: %d", detRes.ProcessingTimeMs)

	// 2. Zero-page validation in both ProcessPdf and ExtractPagesMarkdown
	_, err = pdfinspector.ProcessPdf(pdfData, &pdfinspector.ProcessOptions{Pages: []uint32{0}})
	if err == nil || !strings.Contains(err.Error(), "1-indexed") {
		t.Errorf("Expected error for page index 0 in ProcessPdf, got: %v", err)
	}

	_, err = pdfinspector.ExtractPagesMarkdown(pdfData, []uint32{0})
	if err == nil || !strings.Contains(err.Error(), "1-indexed") {
		t.Errorf("Expected error for page index 0 in ExtractPagesMarkdown, got: %v", err)
	}

	// 3. 1-indexed page selection consistency (page 1 = first page)
	page1Res, err := pdfinspector.ExtractPagesMarkdown(pdfData, []uint32{1})
	if err != nil {
		t.Fatalf("ExtractPagesMarkdown for page 1 failed: %v", err)
	}
	if len(page1Res.Pages) != 1 || page1Res.Pages[0].Page != 1 {
		t.Errorf("Expected 1-indexed page 1 result, got: %+v", page1Res.Pages)
	}

	// 4. Invalid profile validation
	_, err = pdfinspector.ProcessPdf(pdfData, &pdfinspector.ProcessOptions{Profile: "invalid_profile"})
	if err == nil || !strings.Contains(err.Error(), "Invalid markdown profile") {
		t.Errorf("Expected error for invalid profile, got: %v", err)
	}

	// 5. Empty slice/nil page selection consistency across APIs
	pagesRes, err := pdfinspector.ExtractPagesMarkdown(pdfData, []uint32{})
	if err != nil {
		t.Fatalf("ExtractPagesMarkdown with empty slice failed: %v", err)
	}
	if len(pagesRes.Pages) == 0 {
		t.Errorf("Expected empty pages slice to process all pages, got 0 pages")
	}
}

func TestExtractPageGeometry(t *testing.T) {
	pdfData, err := os.ReadFile("tests/fixtures/thermo-freon12.pdf")
	if err != nil {
		t.Fatalf("Failed to read fixture: %v", err)
	}

	pages, err := pdfinspector.ExtractPageGeometry(pdfData)
	if err != nil {
		t.Fatalf("ExtractPageGeometry failed: %v", err)
	}
	if len(pages) == 0 {
		t.Fatal("Expected at least one page")
	}

	page1 := pages[0]
	if page1.Page != 1 {
		t.Errorf("Expected first page to be 1-indexed as 1, got %d", page1.Page)
	}
	if len(page1.TextItems) == 0 {
		t.Errorf("Expected page 1 to have text items")
	}
	for _, item := range page1.TextItems {
		if item.ItemType == "" {
			t.Errorf("Expected non-empty ItemType on text item %+v", item)
		}
	}
}

func TestExtractPageGeometryEmptyBuffer(t *testing.T) {
	_, err := pdfinspector.ExtractPageGeometry(nil)
	if err == nil {
		t.Fatal("Expected error for empty/nil PDF buffer")
	}
}

func TestExtractPageGeometryTableCellOccupancy(t *testing.T) {
	// A fixture with vector-drawn tables, so Source == "Rects" and
	// CellOccupancy are exercised over the WASM boundary rather than only in
	// the Rust unit tests.
	pdfData, err := os.ReadFile("tests/fixtures/wired_header_data_misalign.pdf")
	if err != nil {
		t.Fatalf("Failed to read fixture: %v", err)
	}

	pages, err := pdfinspector.ExtractPageGeometry(pdfData)
	if err != nil {
		t.Fatalf("ExtractPageGeometry failed: %v", err)
	}

	// Counters, so the assertions below cannot pass vacuously: this test used
	// to be satisfied by a run that detected no tables at all.
	rectTablesWithOccupancy := 0
	occupancyCells := 0

	for _, page := range pages {
		for _, table := range page.Tables {
			if table.Source == "Rects" && table.CellOccupancy != nil {
				rectTablesWithOccupancy++
				for _, row := range table.CellOccupancy {
					occupancyCells += len(row)
				}
			}
			switch table.Source {
			case "Rects", "Lines", "Struct", "Heuristic", "Unspecified":
				// expected values
			default:
				t.Errorf("Unexpected TableSource %q", table.Source)
			}
			if table.Source == "Heuristic" && table.CellOccupancy != nil {
				t.Errorf("Heuristic-sourced table must not report CellOccupancy (no real evidence), got %+v", table.CellOccupancy)
			}
			if table.CellOccupancy != nil && len(table.CellOccupancy) != len(table.Cells) {
				t.Errorf("CellOccupancy row count %d must match Cells row count %d", len(table.CellOccupancy), len(table.Cells))
			}
		}
	}

	if rectTablesWithOccupancy == 0 {
		t.Fatalf("fixture produced no rect-detected table carrying CellOccupancy — " +
			"the assertions above never ran against real evidence")
	}
	if occupancyCells == 0 {
		t.Fatalf("CellOccupancy grids were all empty across %d rect table(s)", rectTablesWithOccupancy)
	}
}

// TestExtractPageGeometryPageMetadata covers the page-level geometry fields --
// Rotation and PageBox -- over the real WASM boundary rather than only in Rust.
func TestExtractPageGeometryPageMetadata(t *testing.T) {
	pdfData, err := os.ReadFile("tests/fixtures/thermo-freon12.pdf")
	if err != nil {
		t.Fatalf("Failed to read fixture: %v", err)
	}

	pages, err := pdfinspector.ExtractPageGeometry(pdfData)
	if err != nil {
		t.Fatalf("ExtractPageGeometry failed: %v", err)
	}
	if len(pages) == 0 {
		t.Fatal("Expected at least one page")
	}

	for _, page := range pages {
		if page.PageBox == nil {
			t.Errorf("page %d: expected a PageBox to be reported", page.Page)
			continue
		}
		pb := *page.PageBox
		if pb[2] <= pb[0] || pb[3] <= pb[1] {
			t.Errorf("page %d: PageBox must be normalized (x0<x1, y0<y1), got %v", page.Page, pb)
		}
		// This fixture is an ordinary upright document: no page of it should
		// have had its frame turned.
		if page.Rotation != "Upright" {
			t.Errorf("page %d: unexpected Rotation %q on an upright fixture", page.Page, page.Rotation)
		}
		// Rotation and coordinates are reported in one frame, so an upright
		// page's items must all read as horizontal.
		for _, item := range page.TextItems {
			if item.Rotation < -0.5 || item.Rotation > 0.5 {
				t.Errorf("page %d: upright page has item %q with rotation %f", page.Page, item.Text, item.Rotation)
			}
		}
		// LinkURL must be populated exactly when ItemType == "Link".
		for _, item := range page.TextItems {
			if item.ItemType != "Link" && item.LinkURL != "" {
				t.Errorf("page %d: non-Link item %q carries LinkURL %q", page.Page, item.Text, item.LinkURL)
			}
		}
	}
}

// TestExtractPageGeometryNonTextItemsKeepZeroRotation pins, end to end through
// the WASM boundary, that non-text items report 0 rotation even on a page
// whose frame was turned: only text runs have a baseline to rebase. The Rust
// side guards the same property in
// `content_stream::tests::image_placeholders_keep_zero_rotation_on_rotated_pages`.
func TestExtractPageGeometryNonTextItemsKeepZeroRotation(t *testing.T) {
	fixtures := []string{
		"tests/fixtures/rotated_margin_stamp.pdf",
		"tests/fixtures/text_page_with_watermark_image.pdf",
		"tests/fixtures/firecrawl_docs_tagged.pdf",
	}
	sawNonText := false
	for _, fixture := range fixtures {
		pdfData, err := os.ReadFile(fixture)
		if err != nil {
			t.Fatalf("Failed to read %s: %v", fixture, err)
		}
		pages, err := pdfinspector.ExtractPageGeometry(pdfData)
		if err != nil {
			t.Fatalf("ExtractPageGeometry(%s) failed: %v", fixture, err)
		}
		for _, page := range pages {
			for _, item := range page.TextItems {
				if item.ItemType == "Text" {
					continue
				}
				sawNonText = true
				if item.Rotation != 0 {
					t.Errorf("%s page %d: %s item %q reports rotation %f, want 0",
						fixture, page.Page, item.ItemType, item.Text, item.Rotation)
				}
			}
			// Images are surfaced separately too; their placeholders are the
			// same items, so the same rule holds for the list they come from.
			for _, img := range page.Images {
				if img.Width <= 0 || img.Height <= 0 {
					t.Errorf("%s page %d: image %q has non-positive extents %fx%f",
						fixture, page.Page, img.XObjectName, img.Width, img.Height)
				}
			}
		}
	}
	if !sawNonText {
		t.Fatal("no non-text items found in any fixture; the assertion never ran")
	}
}
