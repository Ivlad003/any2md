use crate::converter::pdf::extractor::{RawElement, RawPage, RawTextBlock};
use crate::model::document::Element;
use tracing::debug;

const Y_LINE_TOLERANCE: f64 = 3.0;
const MIN_TABLE_COLUMNS: usize = 3;
const MIN_TABLE_ROWS: usize = 2;
/// Minimum X-range for a Y-line to be considered "wide" (spanning multiple columns)
const MIN_WIDE_X_RANGE: f64 = 200.0;
/// Minimum number of blocks in a Y-line to consider it potentially tabular
const MIN_BLOCKS_PER_WIDE_LINE: usize = 3;

pub struct DetectedTable {
    pub y_position: f64,
    pub element: Element,
}

pub struct TableDetectionResult {
    pub remaining_elements: Vec<RawElement>,
    pub tables: Vec<DetectedTable>,
}

pub struct TableDetector;

#[derive(Debug, Clone)]
struct Column {
    mean_x: f64,
}

#[derive(Debug, Clone)]
struct YLine {
    mean_y: f64,
    block_indices: Vec<usize>,
}

impl TableDetector {
    pub fn detect(page: &RawPage) -> TableDetectionResult {
        let text_blocks: Vec<(usize, &RawTextBlock)> = page
            .elements
            .iter()
            .enumerate()
            .filter_map(|(i, el)| match el {
                RawElement::Text(b) => Some((i, b)),
                _ => None,
            })
            .collect();

        if text_blocks.len() < 6 {
            return Self::no_table(page);
        }

        // Step 1: Group blocks into visual Y-lines
        let y_lines = Self::group_y_lines(&text_blocks);

        // Step 2: Find the table Y-region by looking for Y-lines with wide X-spread
        let (table_start, table_end) =
            match Self::find_table_y_region(&y_lines, &text_blocks) {
                Some(range) => range,
                None => return Self::no_table(page),
            };

        // Step 3: Collect blocks within the table region, detect columns by text-edge alignment
        let table_y_lines = &y_lines[table_start..=table_end];

        let columns = Self::cluster_x_positions_by_edges(table_y_lines, &text_blocks);
        if columns.len() < MIN_TABLE_COLUMNS {
            return Self::no_table(page);
        }

        // Step 4: Detect rows within the table region
        let rows = Self::detect_rows(table_y_lines, &text_blocks, &columns);
        if rows.len() < MIN_TABLE_ROWS {
            return Self::no_table(page);
        }

        // Step 5: Build the table element
        let table_element = match Self::build_table(&text_blocks, &columns, &rows) {
            Some(el) => el,
            None => return Self::no_table(page),
        };

        let table_y_pos = table_y_lines
            .first()
            .map(|yl| yl.mean_y)
            .unwrap_or(0.0);

        debug!(
            columns = columns.len(),
            header_cols = match &table_element {
                Element::Table { headers, .. } => headers.len(),
                _ => 0,
            },
            data_rows = rows.len().saturating_sub(1),
            "Table detected"
        );

        // Collect original element indices that belong to the table
        let mut table_original_indices = std::collections::HashSet::new();
        for yl in table_y_lines {
            for &bi in &yl.block_indices {
                table_original_indices.insert(text_blocks[bi].0);
            }
        }

        let remaining: Vec<RawElement> = page
            .elements
            .iter()
            .enumerate()
            .filter(|(i, _)| !table_original_indices.contains(i))
            .map(|(_, el)| el.clone())
            .collect();

        TableDetectionResult {
            remaining_elements: remaining,
            tables: vec![DetectedTable {
                y_position: table_y_pos,
                element: table_element,
            }],
        }
    }

    fn no_table(page: &RawPage) -> TableDetectionResult {
        TableDetectionResult {
            remaining_elements: page.elements.clone(),
            tables: vec![],
        }
    }

    /// Find the table Y-region: find contiguous groups of "wide" Y-lines
    /// (blocks spread across a large X range), allowing narrow continuation lines
    /// in between but not large Y gaps.
    fn find_table_y_region(
        y_lines: &[YLine],
        blocks: &[(usize, &RawTextBlock)],
    ) -> Option<(usize, usize)> {
        // Maximum Y gap between consecutive lines to be considered same region
        let max_y_gap = 100.0;

        // Mark which Y-lines are "wide"
        let wide: Vec<bool> = y_lines
            .iter()
            .map(|yl| {
                if yl.block_indices.len() >= MIN_BLOCKS_PER_WIDE_LINE {
                    let xs: Vec<f64> = yl
                        .block_indices
                        .iter()
                        .map(|&bi| blocks[bi].1.x)
                        .collect();
                    let min_x = xs.iter().cloned().fold(f64::INFINITY, f64::min);
                    let max_x = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                    max_x - min_x >= MIN_WIDE_X_RANGE
                } else {
                    false
                }
            })
            .collect();

        // Find contiguous regions of wide lines (allowing narrow continuation
        // lines in between, but breaking at large Y gaps).
        // Region spans from first wide line to last wide line within
        // a Y-gap-limited group.
        let mut best_start = 0;
        let mut best_end = 0;
        let mut best_wide_count = 0;

        let mut region_first_wide: Option<usize> = None;
        let mut region_last_wide: Option<usize> = None;
        let mut region_wide_count = 0;

        let flush_region =
            |first_wide: Option<usize>,
             last_wide: Option<usize>,
             wide_count: usize,
             best_start: &mut usize,
             best_end: &mut usize,
             best_wide_count: &mut usize| {
                if let (Some(start), Some(end)) = (first_wide, last_wide) {
                    if wide_count > *best_wide_count {
                        *best_start = start;
                        *best_end = end;
                        *best_wide_count = wide_count;
                    }
                }
            };

        for i in 0..y_lines.len() {
            if i > 0 {
                let y_gap = (y_lines[i].mean_y - y_lines[i - 1].mean_y).abs();
                if y_gap > max_y_gap {
                    flush_region(
                        region_first_wide,
                        region_last_wide,
                        region_wide_count,
                        &mut best_start,
                        &mut best_end,
                        &mut best_wide_count,
                    );
                    region_first_wide = None;
                    region_last_wide = None;
                    region_wide_count = 0;
                }
            }

            if wide[i] {
                if region_first_wide.is_none() {
                    region_first_wide = Some(i);
                }
                region_last_wide = Some(i);
                region_wide_count += 1;
            }
        }

        flush_region(
            region_first_wide,
            region_last_wide,
            region_wide_count,
            &mut best_start,
            &mut best_end,
            &mut best_wide_count,
        );

        if best_wide_count >= MIN_TABLE_ROWS && best_end > best_start {
            // Extend to include narrow continuation lines after last wide line
            let mut extended_end = best_end;
            for i in (best_end + 1)..y_lines.len() {
                let y_gap = (y_lines[i].mean_y - y_lines[i - 1].mean_y).abs();
                if y_gap > 30.0 {
                    break;
                }
                extended_end = i;
            }
            Some((best_start, extended_end))
        } else {
            None
        }
    }

    /// Detect columns using text-edge alignment (Nurminen/Tabula approach).
    /// For each Y-line, collect left-edge X positions of blocks.
    /// X edges that appear across multiple Y-lines are column boundaries.
    fn cluster_x_positions_by_edges(
        table_y_lines: &[YLine],
        blocks: &[(usize, &RawTextBlock)],
    ) -> Vec<Column> {
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

        columns.sort_by(|a, b| a.mean_x.partial_cmp(&b.mean_x).unwrap_or(std::cmp::Ordering::Equal));

        // Merge columns that are too close (within snap_tolerance * 2.0)
        let mut deduped: Vec<Column> = Vec::new();
        for col in columns {
            if let Some(last) = deduped.last() {
                if (col.mean_x - last.mean_x).abs() < snap_tolerance * 2.0 {
                    continue;
                }
            }
            deduped.push(col);
        }

        deduped
    }

    fn group_y_lines(blocks: &[(usize, &RawTextBlock)]) -> Vec<YLine> {
        if blocks.is_empty() {
            return vec![];
        }

        let mut sorted_indices: Vec<usize> = (0..blocks.len()).collect();
        sorted_indices.sort_by(|&a, &b| {
            blocks[a]
                .1
                .y
                .partial_cmp(&blocks[b].1.y)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut lines: Vec<YLine> = Vec::new();
        let mut current_ys: Vec<f64> = vec![blocks[sorted_indices[0]].1.y];
        let mut current_indices: Vec<usize> = vec![sorted_indices[0]];

        for &si in &sorted_indices[1..] {
            let y = blocks[si].1.y;
            let mean_y: f64 = current_ys.iter().sum::<f64>() / current_ys.len() as f64;
            if (y - mean_y).abs() <= Y_LINE_TOLERANCE {
                current_ys.push(y);
                current_indices.push(si);
            } else {
                let mean = current_ys.iter().sum::<f64>() / current_ys.len() as f64;
                lines.push(YLine {
                    mean_y: mean,
                    block_indices: current_indices.clone(),
                });
                current_ys = vec![y];
                current_indices = vec![si];
            }
        }
        let mean = current_ys.iter().sum::<f64>() / current_ys.len() as f64;
        lines.push(YLine {
            mean_y: mean,
            block_indices: current_indices,
        });

        lines.sort_by(|a, b| a.mean_y.partial_cmp(&b.mean_y).unwrap_or(std::cmp::Ordering::Equal));
        lines
    }

    fn find_column(x: f64, columns: &[Column]) -> Option<usize> {
        columns
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                (x - a.mean_x)
                    .abs()
                    .partial_cmp(&(x - b.mean_x).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .filter(|(_, col)| (x - col.mean_x).abs() < 50.0)
            .map(|(i, _)| i)
    }

    /// Detect rows within the table region.
    /// A new row starts when a Y-line has blocks in >= MIN_TABLE_COLUMNS distinct columns.
    /// Lines with fewer columns are continuation lines of the previous row.
    fn detect_rows<'a>(
        table_y_lines: &'a [YLine],
        blocks: &[(usize, &RawTextBlock)],
        columns: &[Column],
    ) -> Vec<Vec<&'a YLine>> {
        if table_y_lines.is_empty() {
            return vec![];
        }

        let line_col_counts: Vec<usize> = table_y_lines
            .iter()
            .map(|yl| {
                let mut col_set = std::collections::HashSet::new();
                for &bi in &yl.block_indices {
                    let (_, block) = &blocks[bi];
                    if let Some(ci) = Self::find_column(block.x, columns) {
                        col_set.insert(ci);
                    }
                }
                col_set.len()
            })
            .collect();

        let mut rows: Vec<Vec<&YLine>> = Vec::new();
        for (i, yl) in table_y_lines.iter().enumerate() {
            if line_col_counts[i] >= MIN_TABLE_COLUMNS || rows.is_empty() {
                rows.push(vec![yl]);
            } else {
                rows.last_mut().unwrap().push(yl);
            }
        }

        rows
    }

    fn build_table(
        blocks: &[(usize, &RawTextBlock)],
        columns: &[Column],
        rows: &[Vec<&YLine>],
    ) -> Option<Element> {
        if rows.len() < MIN_TABLE_ROWS {
            return None;
        }

        let num_cols = columns.len();

        let mut all_rows: Vec<Vec<String>> = Vec::new();
        for row in rows {
            let mut cells: Vec<Vec<String>> = vec![Vec::new(); num_cols];
            for yl in row {
                // Sort block indices by X within each Y-line for proper text ordering
                let mut sorted_bis: Vec<usize> = yl.block_indices.clone();
                sorted_bis.sort_by(|&a, &b| {
                    blocks[a]
                        .1
                        .x
                        .partial_cmp(&blocks[b].1.x)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                for &bi in &sorted_bis {
                    let (_, block) = &blocks[bi];
                    if let Some(ci) = Self::find_column(block.x, columns) {
                        cells[ci].push(block.text.clone());
                    }
                }
            }
            let row_strings: Vec<String> = cells
                .into_iter()
                .map(|texts| {
                    let joined = texts.join(" ");
                    joined.replace('|', "\\|")
                })
                .collect();
            all_rows.push(row_strings);
        }

        if all_rows.is_empty() {
            return None;
        }

        let headers = all_rows.remove(0);

        Some(Element::Table {
            headers,
            rows: all_rows,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::converter::pdf::extractor::RawImage;

    fn make_block(text: &str, x: f64, y: f64) -> RawTextBlock {
        let end_x = x + text.chars().count() as f64 * 14.7 * 0.5;
        RawTextBlock {
            text: text.to_string(),
            x,
            y,
            end_x,
            font_size: 14.7,
            font_name: "Helvetica".to_string(),
            has_bold: false,
            has_italic: false,
        }
    }

    fn make_page(blocks: Vec<RawTextBlock>) -> RawPage {
        RawPage {
            elements: blocks.into_iter().map(RawElement::Text).collect(),
        }
    }

    #[test]
    fn test_no_table_when_too_few_blocks() {
        let page = make_page(vec![
            make_block("Hello", 50.0, 100.0),
            make_block("World", 50.0, 120.0),
        ]);
        let result = TableDetector::detect(&page);
        assert!(result.tables.is_empty());
        assert_eq!(result.remaining_elements.len(), 2);
    }

    #[test]
    fn test_no_table_when_narrow_layout() {
        // All blocks are within a narrow X range — not a table
        let page = make_page(vec![
            make_block("Line 1", 50.0, 100.0),
            make_block("Line 2", 50.0, 120.0),
            make_block("Line 3", 80.0, 140.0),
            make_block("Line 4", 50.0, 160.0),
            make_block("Line 5", 80.0, 180.0),
            make_block("Line 6", 50.0, 200.0),
        ]);
        let result = TableDetector::detect(&page);
        assert!(result.tables.is_empty());
    }

    #[test]
    fn test_simple_table_detected() {
        let page = make_page(vec![
            // Header row: 4 blocks spanning 30..300 (range = 270 > 200)
            make_block("#", 30.0, 100.0),
            make_block("Name", 100.0, 100.0),
            make_block("Type", 250.0, 100.0),
            make_block("Notes", 400.0, 100.0),
            // Row 1
            make_block("1", 30.0, 140.0),
            make_block("Foo", 100.0, 140.0),
            make_block("String", 250.0, 140.0),
            make_block("A note", 400.0, 140.0),
            // Row 2
            make_block("2", 30.0, 180.0),
            make_block("Bar", 100.0, 180.0),
            make_block("Int", 250.0, 180.0),
            make_block("Another", 400.0, 180.0),
        ]);
        let result = TableDetector::detect(&page);
        assert_eq!(result.tables.len(), 1);
        if let Element::Table { headers, rows } = &result.tables[0].element {
            assert_eq!(headers.len(), 4);
            assert_eq!(headers[0], "#");
            assert_eq!(headers[1], "Name");
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0][1], "Foo");
            assert_eq!(rows[1][2], "Int");
        } else {
            panic!("Expected Table element");
        }
        assert!(result.remaining_elements.is_empty());
    }

    #[test]
    fn test_multiline_cells() {
        let page = make_page(vec![
            // Header
            make_block("#", 30.0, 100.0),
            make_block("Name", 150.0, 100.0),
            make_block("Desc", 350.0, 100.0),
            // Row 1 (multiline in Desc column)
            make_block("1", 30.0, 140.0),
            make_block("Foo", 150.0, 140.0),
            make_block("First line", 350.0, 140.0),
            make_block("Second line", 350.0, 157.0),
            // Row 2
            make_block("2", 30.0, 200.0),
            make_block("Bar", 150.0, 200.0),
            make_block("Simple", 350.0, 200.0),
        ]);
        let result = TableDetector::detect(&page);
        assert_eq!(result.tables.len(), 1);
        if let Element::Table { headers, rows } = &result.tables[0].element {
            assert_eq!(headers.len(), 3);
            assert_eq!(rows.len(), 2);
            assert!(rows[0][2].contains("First line"));
            assert!(rows[0][2].contains("Second line"));
        } else {
            panic!("Expected Table element");
        }
    }

    #[test]
    fn test_non_table_elements_preserved() {
        let elements = vec![
            // Non-table paragraph (narrow, won't be part of table)
            RawElement::Text(make_block("Title", 50.0, 50.0)),
            // Table rows (wide spread)
            RawElement::Text(make_block("#", 30.0, 100.0)),
            RawElement::Text(make_block("Name", 150.0, 100.0)),
            RawElement::Text(make_block("Type", 350.0, 100.0)),
            RawElement::Text(make_block("1", 30.0, 140.0)),
            RawElement::Text(make_block("Foo", 150.0, 140.0)),
            RawElement::Text(make_block("String", 350.0, 140.0)),
            RawElement::Text(make_block("2", 30.0, 180.0)),
            RawElement::Text(make_block("Bar", 150.0, 180.0)),
            RawElement::Text(make_block("Int", 350.0, 180.0)),
            // Non-table after
            RawElement::Text(make_block("Footer", 50.0, 250.0)),
            RawElement::Image(RawImage {
                data: vec![0xFF],
                width: 10,
                height: 10,
            }),
        ];
        let page = RawPage { elements };
        let result = TableDetector::detect(&page);
        assert_eq!(result.tables.len(), 1);
        let remaining_texts: Vec<&str> = result
            .remaining_elements
            .iter()
            .filter_map(|el| match el {
                RawElement::Text(b) => Some(b.text.as_str()),
                _ => None,
            })
            .collect();
        assert!(remaining_texts.contains(&"Title"));
        assert!(remaining_texts.contains(&"Footer"));
        assert_eq!(
            result
                .remaining_elements
                .iter()
                .filter(|el| matches!(el, RawElement::Image(_)))
                .count(),
            1
        );
    }

    #[test]
    fn test_pipe_character_escaped() {
        let page = make_page(vec![
            make_block("Col1", 30.0, 100.0),
            make_block("Col2", 150.0, 100.0),
            make_block("Col3", 350.0, 100.0),
            make_block("a|b", 30.0, 140.0),
            make_block("c", 150.0, 140.0),
            make_block("d", 350.0, 140.0),
        ]);
        let result = TableDetector::detect(&page);
        assert_eq!(result.tables.len(), 1);
        if let Element::Table { rows, .. } = &result.tables[0].element {
            assert_eq!(rows[0][0], "a\\|b");
        }
    }

    #[test]
    fn test_x_clustering() {
        // Use cluster_x_positions_by_edges with multiple Y-lines so edges
        // appear enough times to pass the min_appearances threshold.
        let b1 = make_block("a", 30.0, 100.0);
        let b2 = make_block("b", 32.0, 100.0);
        let b3 = make_block("c", 100.0, 100.0);
        let b4 = make_block("d", 105.0, 100.0);
        let b5 = make_block("e", 250.0, 100.0);
        // Second Y-line with same X edges
        let b6 = make_block("f", 30.0, 140.0);
        let b7 = make_block("g", 100.0, 140.0);
        let b8 = make_block("h", 250.0, 140.0);
        let blocks = vec![
            (0, &b1), (1, &b2), (2, &b3), (3, &b4), (4, &b5),
            (5, &b6), (6, &b7), (7, &b8),
        ];
        let y_lines = vec![
            YLine {
                mean_y: 100.0,
                block_indices: vec![0, 1, 2, 3, 4],
            },
            YLine {
                mean_y: 140.0,
                block_indices: vec![5, 6, 7],
            },
        ];
        let columns = TableDetector::cluster_x_positions_by_edges(&y_lines, &blocks);
        assert_eq!(columns.len(), 3);
    }

    #[test]
    fn test_table_includes_continuation_rows_after_last_wide() {
        let page = make_page(vec![
            // Wide row 1 (header): 3 blocks spanning 30..400 (range = 370 > 200)
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
            assert!(
                all_text.contains("F more"),
                "Expected 'F more' in last row, got: {:?}",
                last_row
            );
        } else {
            panic!("Expected Table");
        }
    }

    #[test]
    fn test_narrow_columns_detected() {
        // Simulate real PDF: columns at x=30, 86, 216.7, 331.4, 470.3, 669.3, 1080.3, 1411.0
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
            assert_eq!(
                headers.len(),
                8,
                "Expected 8 columns, got {}: {:?}",
                headers.len(),
                headers
            );
            assert_eq!(headers[0], "#");
            assert_eq!(rows.len(), 2);
        } else {
            panic!("Expected Table");
        }
    }
}
