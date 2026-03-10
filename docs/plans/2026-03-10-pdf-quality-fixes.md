# PDF-to-Markdown Quality Fixes Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix all 16 identified output quality issues in the PDF-to-Markdown converter by improving text block width tracking, gap-based space insertion, table column detection, heading merging, and bold detection.

**Architecture:** The fixes span 4 files in the pipeline: extractor.rs (width tracking + gap-based spaces), table_detector.rs (text-edge column detection), classifier.rs (heading merge), assembler.rs (multi-segment bold). Each task is independent enough to test in isolation. Approach follows pdfminer/Tabula best practices: character-relative thresholds, text-edge clustering, whitespace correlation.

**Tech Stack:** Rust, lopdf, existing any2md pipeline

---

### Task 1: Add `end_x` field to RawTextBlock for width tracking

Currently `RawTextBlock` only stores the starting `x` position. Without knowing where a block ends, we can't compute gaps between adjacent blocks. This is required by Tasks 2 and 3.

**Files:**
- Modify: `src/converter/pdf/extractor.rs:8-14` (RawTextBlock struct)
- Modify: `src/converter/pdf/extractor.rs:372-558` (extract_page_from_streams — track advancing x per character)
- Modify: `src/converter/pdf/extractor.rs:840-863` (extract_page_fallback)

**Step 1: Add `end_x` field to `RawTextBlock`**

In `src/converter/pdf/extractor.rs`, add `end_x` to the struct:

```rust
#[derive(Debug, Clone)]
pub struct RawTextBlock {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub end_x: f64,   // <-- NEW: right edge of text
    pub font_size: f64,
    pub font_name: String,
}
```

**Step 2: Track end_x in extract_page_from_streams**

Each Td advances the text position. The current `tm_x` after processing a Tj/TJ is where the next character would go — that's our `end_x`. After every Tj/TJ/'/", store `end_x` using the current tm_x + the advance from Td.

Key insight: For CID-encoded fonts, each character is a separate `Td + Tj` pair. The `Td` before the NEXT character tells us the width of the CURRENT character. So `end_x` should be the tm_x value at the point of the NEXT Td (or ET).

Simpler approach: track `last_td_x` — the accumulated X after each Td. When we create a RawTextBlock, `end_x = last_td_x + estimated_last_char_width`. For the last char, estimate width as `font_size * 0.6` (average char width for Helvetica).

Actually, the simplest robust approach: after creating a block from Tj, set `end_x = tm_x + text.chars().count() as f64 * font_size * 0.5`. This is a rough estimate but sufficient for gap detection. Later tasks can refine.

In each Tj/TJ/'/\" handler, when creating the RawTextBlock:
```rust
let char_count = final_text.trim().chars().count() as f64;
let estimated_width = char_count * current_font_size.abs() * 0.5;
elements.push(RawElement::Text(RawTextBlock {
    text: final_text,
    x: tm_x,
    y: tm_y,
    end_x: tm_x + estimated_width,
    font_size: current_font_size.abs(),
    font_name,
}));
```

**Step 3: Fix all other RawTextBlock constructors**

Update `extract_page_fallback` (line ~850):
```rust
let estimated_width = trimmed.chars().count() as f64 * 12.0 * 0.5;
elements.push(RawElement::Text(RawTextBlock {
    text: trimmed.to_string(),
    x: 72.0,
    y: y_pos,
    end_x: 72.0 + estimated_width,
    font_size: 12.0,
    font_name: "Unknown".to_string(),
}));
```

**Step 4: Fix all test helpers that create RawTextBlock**

In `extractor.rs` tests, `table_detector.rs` tests, `classifier.rs` tests, `assembler.rs` tests — add `end_x` field wherever `RawTextBlock` is constructed.

For test helpers like `make_block(text, x, y)`, compute `end_x`:
```rust
fn make_block(text: &str, x: f64, y: f64) -> RawTextBlock {
    let end_x = x + text.chars().count() as f64 * 14.7 * 0.5;
    RawTextBlock {
        text: text.to_string(),
        x,
        y,
        end_x,
        font_size: 14.7,
        font_name: "Helvetica".to_string(),
    }
}
```

**Step 5: Update merge_text_blocks to maintain end_x**

In `merge_text_blocks`, when merging blocks, update `prev.end_x` to the new block's `end_x`:
```rust
if same_font {
    prev.text.push_str(&block.text);
    prev.end_x = block.end_x;  // <-- track right edge
    continue;
}
// ... cross-font merge:
prev.end_x = block.end_x;  // <-- also here
```

**Step 6: Run tests**

Run: `cargo test 2>&1`
Expected: All existing tests pass (with `end_x` added to all constructors).

**Step 7: Commit**

```bash
git add src/converter/pdf/extractor.rs src/converter/pdf/table_detector.rs src/converter/pdf/classifier.rs src/converter/pdf/assembler.rs
git commit -m "feat: add end_x field to RawTextBlock for width tracking"
```

---

### Task 2: Gap-based space insertion in merge_text_blocks

Currently `merge_text_blocks` uses font-change heuristic (always insert space between different fonts). This is wrong for same-font blocks that are in different table columns. Use gap-based detection following the pdfminer approach: `gap > char_width * threshold`.

**Files:**
- Modify: `src/converter/pdf/extractor.rs:207-268` (merge_text_blocks)

**Step 1: Write failing test**

In `src/converter/pdf/extractor.rs` test module, add:

```rust
#[test]
fn test_merge_blocks_gap_based_space() {
    // Two blocks far apart on same line should get a space
    let elements = vec![
        RawElement::Text(RawTextBlock {
            text: "Hello".to_string(),
            x: 50.0, y: 100.0, end_x: 80.0,
            font_size: 12.0, font_name: "Helvetica".to_string(),
        }),
        RawElement::Text(RawTextBlock {
            text: "World".to_string(),
            x: 120.0, y: 100.0, end_x: 150.0,
            font_size: 12.0, font_name: "Helvetica".to_string(),
        }),
    ];
    let merged = PdfExtractor::merge_text_blocks(elements);
    assert_eq!(merged.len(), 1);
    if let RawElement::Text(b) = &merged[0] {
        assert_eq!(b.text, "Hello World");
    }
}

#[test]
fn test_merge_blocks_large_gap_separates() {
    // Two blocks VERY far apart (different columns) should NOT merge
    let elements = vec![
        RawElement::Text(RawTextBlock {
            text: "Col1".to_string(),
            x: 50.0, y: 100.0, end_x: 80.0,
            font_size: 12.0, font_name: "Helvetica".to_string(),
        }),
        RawElement::Text(RawTextBlock {
            text: "Col2".to_string(),
            x: 400.0, y: 100.0, end_x: 430.0,
            font_size: 12.0, font_name: "Helvetica".to_string(),
        }),
    ];
    let merged = PdfExtractor::merge_text_blocks(elements);
    assert_eq!(merged.len(), 2);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test test_merge_blocks_gap_based_space test_merge_blocks_large_gap_separates -- --nocapture 2>&1`
Expected: At least `test_merge_blocks_large_gap_separates` FAILS (currently all same-line same-font blocks merge unconditionally).

**Step 3: Implement gap-based merge logic**

Replace the merge logic in `merge_text_blocks`. The key algorithm (from pdfminer):
- Compute gap = `block.x - prev.end_x`
- Compute average char width = `prev.font_size * 0.5` (Helvetica average)
- If gap > `char_width * 4.0` → separate blocks (different columns)
- If gap > `char_width * 0.3` → insert space (word boundary)
- If gap <= `char_width * 0.3` → merge directly (kerning)

```rust
fn merge_text_blocks(elements: Vec<RawElement>) -> Vec<RawElement> {
    let mut merged: Vec<RawElement> = Vec::new();

    for el in elements {
        match el {
            RawElement::Text(block) => {
                if let Some(RawElement::Text(ref mut prev)) = merged.last_mut() {
                    let same_line = (prev.y - block.y).abs() < 1.0;
                    if same_line {
                        let gap = block.x - prev.end_x;
                        let avg_char_width = prev.font_size * 0.5;

                        // Large gap = separate blocks (different table columns)
                        if gap > avg_char_width * 4.0 {
                            merged.push(RawElement::Text(block));
                            continue;
                        }

                        // Medium gap or different font = insert space
                        let same_font = prev.font_name == block.font_name
                            && (prev.font_size - block.font_size).abs() < 0.1;
                        let needs_space = !same_font
                            || gap > avg_char_width * 0.3;

                        if needs_space
                            && !block.text.starts_with(' ')
                            && !prev.text.ends_with(' ')
                        {
                            prev.text.push(' ');
                        }
                        prev.text.push_str(&block.text);
                        prev.end_x = block.end_x;
                        // Track latest font for subsequent merges
                        if !same_font {
                            prev.font_name = block.font_name;
                            prev.font_size = block.font_size;
                        }
                        continue;
                    }
                }
                merged.push(RawElement::Text(block));
            }
            other => merged.push(other),
        }
    }

    // Trim and filter empty
    for el in &mut merged {
        if let RawElement::Text(ref mut b) = el {
            b.text = b.text.trim().to_string();
        }
    }
    merged.into_iter().filter(|el| {
        if let RawElement::Text(ref b) = el { !b.text.is_empty() } else { true }
    }).collect()
}
```

**Step 4: Run tests**

Run: `cargo test 2>&1`
Expected: ALL tests pass including the two new ones.

**Step 5: Commit**

```bash
git add src/converter/pdf/extractor.rs
git commit -m "feat: gap-based space insertion using pdfminer approach"
```

---

### Task 3: Text-edge column detection in table_detector

Replace the fixed 45pt X_CLUSTER_TOLERANCE with text-edge alignment detection (Nurminen/Tabula approach). Count how many Y-lines share the same left-edge X positions. Frequently shared edges = column boundaries.

**Files:**
- Modify: `src/converter/pdf/table_detector.rs:5` (remove X_CLUSTER_TOLERANCE constant)
- Modify: `src/converter/pdf/table_detector.rs:242-333` (cluster_x_positions)
- Modify: `src/converter/pdf/extractor.rs` (needs `end_x` from Task 1)

**Step 1: Write failing test**

Add to `table_detector.rs` tests:

```rust
#[test]
fn test_narrow_columns_detected() {
    // Simulate the real PDF's header: columns at x=30, 86, 216, 331, 470, 669, 1080, 1308, 1411
    // With old 45pt tolerance, 30 and 86 would merge. With text-edge, they should be separate.
    let page = make_page(vec![
        // Header row
        make_block("#", 30.0, 100.0),
        make_block("Liste", 86.0, 100.0),
        make_block("List of", 216.7, 100.0),
        make_block("Type", 331.4, 100.0),
        make_block("Rules", 470.3, 100.0),
        make_block("FR col", 669.3, 100.0),
        make_block("Regles", 1080.3, 100.0),
        make_block("Retour", 1411.0, 100.0),
        // Data row 1
        make_block("1", 30.0, 140.0),
        make_block("Name", 86.0, 140.0),
        make_block("Type A", 216.7, 140.0),
        make_block("Tab", 331.4, 140.0),
        make_block("Rule1", 470.3, 140.0),
        make_block("(1) text", 669.3, 140.0),
        make_block("Regle1", 1080.3, 140.0),
        make_block("ok", 1411.0, 140.0),
        // Data row 2
        make_block("2", 30.0, 180.0),
        make_block("Name2", 86.0, 180.0),
        make_block("Type B", 216.7, 180.0),
        make_block("CTA", 331.4, 180.0),
        make_block("Rule2", 470.3, 180.0),
        make_block("(2) text", 669.3, 180.0),
        make_block("Regle2", 1080.3, 180.0),
        make_block("ok", 1411.0, 180.0),
    ]);
    let result = TableDetector::detect(&page);
    assert_eq!(result.tables.len(), 1);
    if let Element::Table { headers, rows } = &result.tables[0].element {
        // Should detect 8 separate columns, not fewer
        assert!(headers.len() >= 7, "Expected >= 7 columns, got {}", headers.len());
        assert_eq!(headers[0], "#");
        assert_eq!(rows.len(), 2);
    } else {
        panic!("Expected Table");
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_narrow_columns_detected -- --nocapture 2>&1`
Expected: FAIL — old clustering merges columns at 30 and 86 (gap = 56, just above 45pt tolerance, but real issue is it can't distinguish the 8 distinct column edges).

**Step 3: Implement text-edge column detection**

Replace `cluster_x_positions` with a text-edge approach:

```rust
/// Detect columns using text-edge alignment (Nurminen/Tabula approach).
/// For each Y-line, collect left-edge X positions of blocks.
/// X edges that appear across multiple Y-lines are column boundaries.
fn cluster_x_positions_by_edges(
    table_y_lines: &[YLine],
    blocks: &[(usize, &RawTextBlock)],
) -> Vec<Column> {
    // Snap tolerance: edges within this range are considered the same
    let snap_tolerance = 8.0;

    // Collect all left-edge X positions, snapped to grid
    let mut edge_counts: std::collections::BTreeMap<i64, (f64, usize)> =
        std::collections::BTreeMap::new();

    let num_y_lines = table_y_lines.len();

    for yl in table_y_lines {
        // Deduplicate X edges within the same Y-line
        let mut line_edges: Vec<f64> = Vec::new();
        for &bi in &yl.block_indices {
            let x = blocks[bi].1.x;
            // Check if this edge is already counted for this line
            if !line_edges.iter().any(|&e| (e - x).abs() < snap_tolerance) {
                line_edges.push(x);
            }
        }
        for x in line_edges {
            let key = (x / snap_tolerance).round() as i64;
            let entry = edge_counts.entry(key).or_insert((0.0, 0));
            entry.0 += x;
            entry.1 += 1;
        }
    }

    // Keep edges that appear in at least 30% of Y-lines (or at least 2 lines)
    let min_appearances = (num_y_lines as f64 * 0.3).ceil().max(2.0) as usize;

    let mut columns: Vec<Column> = edge_counts
        .values()
        .filter(|(_, count)| *count >= min_appearances)
        .map(|(sum, count)| Column {
            mean_x: sum / *count as f64,
        })
        .collect();

    columns.sort_by(|a, b| a.mean_x.partial_cmp(&b.mean_x).unwrap());

    // Merge columns that are too close (within snap_tolerance)
    let mut deduped: Vec<Column> = Vec::new();
    for col in columns {
        if let Some(last) = deduped.last() {
            if (col.mean_x - last.mean_x).abs() < snap_tolerance * 2.0 {
                continue; // skip duplicate
            }
        }
        deduped.push(col);
    }

    deduped
}
```

Update the `detect()` method to call the new function (pass `table_y_lines` and `text_blocks` to it instead of just `table_blocks_for_clustering`).

**Step 4: Run all tests**

Run: `cargo test 2>&1`
Expected: ALL pass.

**Step 5: Commit**

```bash
git add src/converter/pdf/table_detector.rs
git commit -m "feat: text-edge column detection using Nurminen/Tabula approach"
```

---

### Task 4: Multi-line heading merge in assembler

When consecutive blocks are classified as the same heading level and have close Y positions, merge them into a single heading. This fixes: `## Client - Predefined product list (To review by` / `## PICTO)`.

**Files:**
- Modify: `src/converter/pdf/assembler.rs:34-41` (Heading case in assemble_page)

**Step 1: Write failing test**

In `assembler.rs` tests:

```rust
#[test]
fn test_consecutive_headings_same_level_merged() {
    let b1 = RawTextBlock {
        text: "Client - Predefined product list (To review by".to_string(),
        x: 50.0, y: 700.0, end_x: 600.0,
        font_size: 26.7, font_name: "Helvetica".to_string(),
    };
    let b2 = RawTextBlock {
        text: "PICTO)".to_string(),
        x: 50.0, y: 720.0, end_x: 130.0,
        font_size: 26.7, font_name: "Helvetica".to_string(),
    };
    let blocks = vec![
        ClassifiedElement::Text(b1, BlockType::Heading(2)),
        ClassifiedElement::Text(b2, BlockType::Heading(2)),
    ];
    let doc = Assembler::assemble(vec![blocks], empty_metadata());
    assert_eq!(doc.pages[0].elements.len(), 1);
    if let Element::Heading { level, text } = &doc.pages[0].elements[0] {
        assert_eq!(*level, 2);
        assert!(text.contains("Client"));
        assert!(text.contains("PICTO)"));
    } else {
        panic!("Expected merged heading");
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_consecutive_headings_same_level_merged -- --nocapture 2>&1`
Expected: FAIL — currently produces 2 separate headings.

**Step 3: Implement heading merge**

In `assembler.rs`, modify the `Heading` case in `assemble_page`:

```rust
BlockType::Heading(level) => {
    let mut heading_text = block.text.clone();
    let heading_level = *level;
    i += 1;
    // Merge consecutive headings at the same level (wrapped text)
    while i < elems.len() {
        if let ClassifiedElement::Text(next_block, BlockType::Heading(next_level)) = &elems[i] {
            if *next_level == heading_level {
                // Check Y proximity: next line should be within ~1.5x font height
                let y_gap = (next_block.y - block.y).abs();
                if y_gap < block.font_size * 2.0 {
                    heading_text.push(' ');
                    heading_text.push_str(&next_block.text);
                    i += 1;
                    continue;
                }
            }
        }
        break;
    }
    elements.push(Element::Heading {
        level: heading_level,
        text: heading_text,
    });
}
```

Note: The `block` variable from the outer match is used for Y comparison. Need to track the last heading block's Y for proper gap calculation across multiple continuation lines.

**Step 4: Run tests**

Run: `cargo test 2>&1`
Expected: ALL pass.

**Step 5: Commit**

```bash
git add src/converter/pdf/assembler.rs
git commit -m "feat: merge consecutive same-level headings (wrapped text)"
```

---

### Task 5: Fix bold detection lost after cross-font merge

In `merge_text_blocks`, when different-font blocks merge, `prev.font_name` is overwritten. The assembler uses `font_name` for bold detection, so if the last font is non-bold, the entire paragraph loses bold. Fix: track whether ANY merged block was bold.

**Files:**
- Modify: `src/converter/pdf/extractor.rs:8-14` (add `has_bold` and `has_italic` flags to RawTextBlock)
- Modify: `src/converter/pdf/extractor.rs` (merge_text_blocks — track bold across merges)
- Modify: `src/converter/pdf/assembler.rs:126-136` (use has_bold/has_italic flags)

**Step 1: Add formatting flags to RawTextBlock**

```rust
#[derive(Debug, Clone)]
pub struct RawTextBlock {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub end_x: f64,
    pub font_size: f64,
    pub font_name: String,
    pub has_bold: bool,    // <-- NEW
    pub has_italic: bool,  // <-- NEW
}
```

**Step 2: Set flags during extraction**

When creating RawTextBlock in Tj/TJ handlers, set:
```rust
has_bold: font_name.to_lowercase().contains("bold"),
has_italic: font_name.to_lowercase().contains("italic") || font_name.to_lowercase().contains("oblique"),
```

**Step 3: Propagate during merge**

In `merge_text_blocks`, when merging cross-font blocks:
```rust
prev.has_bold = prev.has_bold || block.has_bold;
prev.has_italic = prev.has_italic || block.has_italic;
```

**Step 4: Use in assembler**

In `assembler.rs:rich_text_from_block`, change:
```rust
fn rich_text_from_block(text: &str, block: &RawTextBlock) -> RichText {
    RichText {
        segments: vec![TextSegment {
            text: text.to_string(),
            bold: block.has_bold,
            italic: block.has_italic,
            code: false,
            link: None,
        }],
    }
}
```

**Step 5: Fix all test constructors**

Add `has_bold: false, has_italic: false` to all test `RawTextBlock` constructors across all test files.

**Step 6: Run tests**

Run: `cargo test 2>&1`
Expected: ALL pass.

**Step 7: Commit**

```bash
git add src/converter/pdf/extractor.rs src/converter/pdf/assembler.rs src/converter/pdf/classifier.rs src/converter/pdf/table_detector.rs
git commit -m "feat: preserve bold/italic flags across cross-font text merges"
```

---

### Task 6: Fix table region boundary to include all continuation rows

Currently the table region detection (`find_table_y_region`) only includes lines between first and last "wide" Y-lines. Content that continues below the last wide line (e.g., row 7's continuation text about "Remove product", "Edit product") is excluded. Fix: extend region to include narrow continuation lines after the last wide line, up to the next large Y gap.

**Files:**
- Modify: `src/converter/pdf/table_detector.rs:142-237` (find_table_y_region)

**Step 1: Write failing test**

```rust
#[test]
fn test_table_includes_continuation_rows_after_last_wide() {
    let page = make_page(vec![
        // Wide row 1 (header)
        make_block("Col1", 30.0, 100.0),
        make_block("Col2", 200.0, 100.0),
        make_block("Col3", 400.0, 100.0),
        // Wide row 2
        make_block("A", 30.0, 140.0),
        make_block("B", 200.0, 140.0),
        make_block("C", 400.0, 140.0),
        // Narrow continuation of row 2 (only col3)
        make_block("C continued", 400.0, 157.0),
        // Wide row 3
        make_block("D", 30.0, 200.0),
        make_block("E", 200.0, 200.0),
        make_block("F", 400.0, 200.0),
        // Narrow continuation AFTER last wide row
        make_block("F more", 400.0, 217.0),
    ]);
    let result = TableDetector::detect(&page);
    assert_eq!(result.tables.len(), 1);
    if let Element::Table { rows, .. } = &result.tables[0].element {
        // The continuation line "F more" should be in the last row
        let last_row = rows.last().unwrap();
        let all_text = last_row.join(" ");
        assert!(all_text.contains("F more"), "Expected 'F more' in last row, got: {:?}", last_row);
    } else {
        panic!("Expected Table");
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_table_includes_continuation_rows_after_last_wide 2>&1`

**Step 3: Implement fix**

In `find_table_y_region`, after finding `best_end` (last wide line), extend it to include subsequent non-wide lines that are within a small Y gap (e.g., `font_size * 3`):

```rust
// After computing best_start, best_end...
// Extend best_end to include narrow continuation lines
let mut extended_end = best_end;
for i in (best_end + 1)..y_lines.len() {
    let y_gap = (y_lines[i].mean_y - y_lines[i - 1].mean_y).abs();
    if y_gap > 30.0 {
        break; // Too large a gap — not a continuation
    }
    extended_end = i;
}
// Return extended range
Some((best_start, extended_end))
```

**Step 4: Run tests**

Run: `cargo test 2>&1`
Expected: ALL pass.

**Step 5: Commit**

```bash
git add src/converter/pdf/table_detector.rs
git commit -m "feat: extend table region to include continuation rows after last wide line"
```

---

### Task 7: Fix missing spaces in same-font text (202617:47, 12:03OneNote)

The `merge_text_blocks` gap check now uses `end_x` (from Task 1) and character-relative thresholds (from Task 2). But same-font blocks that were split across BT/ET groups without explicit space CIDs still lack gap detection because `pending_space` doesn't fire.

These specific cases (`202617:47`, `12:03OneNote`) are same-font blocks on the same line where the PDF has no space CID between them — only an X-position gap. Task 2's gap-based merge already handles this for different-font blocks, but we need to also check the gap for same-font blocks.

**This is actually handled by Task 2** — the gap-based merge applies to both same-font and different-font blocks equally. Once `end_x` is tracked (Task 1) and the gap threshold is implemented (Task 2), these cases should be fixed automatically.

**Verification step after Tasks 1+2:**

Run: `cargo run -- test.pdf -o test_output.md 2>&1`
Check: Line 9 should have `2026 17:47` (with space), line 82 should have `12:03 OneNote` (with space).

If not fixed, adjust the gap threshold in Task 2's implementation (may need `avg_char_width * 0.2` instead of `0.3`).

---

### Task 8: Fix URL split across lines

Long URLs get split at PDF line boundaries. In the assembler, detect when consecutive paragraphs form a URL continuation and merge them.

**Files:**
- Modify: `src/converter/pdf/assembler.rs:81-86` (Paragraph case)

**Step 1: Write failing test**

```rust
#[test]
fn test_url_continuation_merged() {
    let b1 = RawTextBlock {
        text: "https://www.example.com/path/to/resource?param=value&node-id=1716-".to_string(),
        x: 50.0, y: 500.0, end_x: 600.0,
        font_size: 14.7, font_name: "Helvetica".to_string(),
        has_bold: false, has_italic: false,
    };
    let b2 = RawTextBlock {
        text: "54696&t=J9IapymHwzyzsPkt-0".to_string(),
        x: 50.0, y: 517.0, end_x: 250.0,
        font_size: 14.7, font_name: "Helvetica".to_string(),
        has_bold: false, has_italic: false,
    };
    let blocks = vec![
        ce(b1, BlockType::Paragraph),
        ce(b2, BlockType::Paragraph),
    ];
    let doc = Assembler::assemble(vec![blocks], empty_metadata());
    // Should merge into one paragraph with the full URL
    assert_eq!(doc.pages[0].elements.len(), 1);
    if let Element::Paragraph { text } = &doc.pages[0].elements[0] {
        let full = &text.segments[0].text;
        assert!(full.contains("1716-54696"));
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_url_continuation_merged 2>&1`
Expected: FAIL — produces 2 separate paragraphs.

**Step 3: Implement URL continuation detection**

In the `Paragraph` case of `assemble_page`, check if current paragraph ends with `-` and looks like a URL, and next paragraph continues it:

```rust
BlockType::Paragraph => {
    let mut para_text = block.text.clone();
    let current_font = block.font_name.clone();
    let mut current_y = block.y;
    i += 1;

    // Merge URL continuations: if current text is/contains a URL ending with '-'
    // and next paragraph continues it at same X with close Y
    while i < elems.len() {
        if let ClassifiedElement::Text(next_block, BlockType::Paragraph) = &elems[i] {
            let y_gap = (next_block.y - current_y).abs();
            let same_x = (next_block.x - block.x).abs() < 5.0;
            let line_height = block.font_size * 1.5;

            // URL continuation: previous ends with '-' and contains "://"
            if same_x && y_gap < line_height && para_text.contains("://") && para_text.ends_with('-') {
                para_text.push_str(&next_block.text);
                current_y = next_block.y;
                i += 1;
                continue;
            }
        }
        break;
    }

    elements.push(Element::Paragraph {
        text: Self::rich_text_from_block(&para_text, block),
    });
}
```

**Step 4: Run tests**

Run: `cargo test 2>&1`
Expected: ALL pass.

**Step 5: Commit**

```bash
git add src/converter/pdf/assembler.rs
git commit -m "feat: merge URL continuations across line breaks"
```

---

### Task 9: Clean up debug example files

Remove the temporary debug examples that are no longer needed.

**Files:**
- Delete: `examples/dump_blocks.rs`
- Delete: `examples/dump_gaps.rs`
- Delete: `examples/dump_ops.rs`

**Step 1: Remove files**

```bash
rm -f examples/dump_blocks.rs examples/dump_gaps.rs examples/dump_ops.rs
```

**Step 2: Verify build**

Run: `cargo build 2>&1`
Expected: No errors.

**Step 3: Commit**

```bash
git add -u examples/
git commit -m "chore: remove temporary debug example files"
```

---

### Task 10: Integration test — run converter on test.pdf and verify output

**Step 1: Run conversion**

```bash
cargo run -- test.pdf -o test_output.md 2>&1
```

**Step 2: Verify fixes**

Check the output against the 16 identified issues:

1. ✅ Heading should be single: `## Client - Predefined product list (To review by PICTO)`
2. ✅ Table header should have separate columns for "List of fields", "Type", "Mandatory?"
3. ✅ Row numbers separate from cell text: `6` | `Liste des produits`
4. ✅ `2026 17:47` with space
5. ✅ `Rules` | `Front behavior` as separate columns
6. ✅ No mid-word column splits
7. ✅ Separate columns not merged into one cell
8. ✅ Same
9. ✅ `"create a new predefined product".` separate from `ok`
10. ✅ `(see Project` not `(sere Project` — this is a CMap issue, may need separate investigation
11. ✅ `12:03 OneNote` with space
12. ✅ `**Status:** To review by PICTO` with bold
13. ✅ Row 7 continuation text inside table
14. ✅ Figma URL on single line
15. ✅ No orphan `8`
16. ✅ French spacing preserved

**Step 3: If issues remain, iterate**

Adjust thresholds:
- Gap-based space threshold (Task 2): try `0.2` instead of `0.3` if spaces still missing
- Column snap tolerance (Task 3): try `5.0` instead of `8.0` if columns still merge
- Table region extension gap (Task 6): try `40.0` instead of `30.0` if rows still excluded

**Step 4: Final commit**

```bash
git add test_output.md
git commit -m "test: verify PDF quality improvements on test document"
```
