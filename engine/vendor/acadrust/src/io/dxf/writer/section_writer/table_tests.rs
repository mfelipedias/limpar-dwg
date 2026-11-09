//! The cells of an ACAD_TABLE are written through an index of their merged ranges built once
//! (`MergedCellIndex`). These tests hold the output to the previous per-cell search, kept here
//! as the reference, and to a time bound.

use super::SectionWriter;
use crate::entities::table::{CellRange, Table, TableCell};
use crate::error::Result;
use crate::io::dxf::writer::stream_writer::DxfStreamWriter;
use crate::io::dxf::writer::text_writer::DxfTextWriter;
use crate::types::Vector3;

impl<W: DxfStreamWriter> SectionWriter<'_, W> {
    /// The previous cell loop: a search of the merged ranges per cell.
    fn write_table_cells_reference(&mut self, table: &Table) -> Result<()> {
        let merged_ranges = table.canonical_merged_ranges();
        for (row_index, row) in table.rows.iter().enumerate() {
            for (column_index, cell) in row.cells.iter().enumerate() {
                let merged_range = merged_ranges
                    .iter()
                    .find(|range| range.contains(row_index, column_index));
                let anchor_range = merged_range
                    .filter(|range| range.top_row == row_index && range.left_col == column_index);
                self.write_table_cell(cell, anchor_range, merged_range.is_some())?;
            }
        }
        Ok(())
    }
}

/// The cells of `table` as DXF text, written by the reference loop or the indexed one.
fn cells_written(table: &Table, reference: bool) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut text = DxfTextWriter::new(&mut buf);
        let mut writer = SectionWriter::new(&mut text, 1, 1);
        let result = if reference {
            writer.write_table_cells_reference(table)
        } else {
            writer.write_table_cells(table)
        };
        result.expect("cells written");
    }
    buf
}

/// Deterministic xorshift64* numbers.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number in `0..n` (`n` > 0).
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// A table of up to 8 × 8 cells: texts, rows longer or shorter than the column count, cell
/// spans and merged ranges (overlapping, duplicated, past the grid).
fn random_table(rng: &mut Rng) -> Table {
    let mut table = Table::new(Vector3::ZERO, rng.below(9), rng.below(9));
    for (r, row) in table.rows.iter_mut().enumerate() {
        if rng.below(6) == 0 {
            let keep = rng.below(row.cells.len() + 1);
            row.cells.truncate(keep);
        }
        if rng.below(6) == 0 {
            row.cells.push(TableCell::new());
        }
        for (c, cell) in row.cells.iter_mut().enumerate() {
            if rng.below(2) == 0 {
                *cell = TableCell::text(&format!("{r},{c}"));
            }
            if rng.below(3) == 0 {
                cell.merge_width = rng.below(5) as i32;
                cell.merge_height = rng.below(5) as i32;
            }
        }
    }
    for _ in 0..rng.below(30) {
        let range = CellRange::new(rng.below(10), rng.below(10), rng.below(10), rng.below(10));
        table.merged_ranges.push(range);
        if rng.below(4) == 0 {
            table.merged_ranges.push(range);
        }
    }
    table
}

#[test]
fn table_cells_are_written_as_before() {
    let mut rng = Rng(0xA076_1D64_78BD_642F);
    let mut merged = 0;
    for case in 0..2_000 {
        let table = random_table(&mut rng);
        if !table.canonical_merged_ranges().is_empty() {
            merged += 1;
        }
        let expected = cells_written(&table, true);
        assert!(
            cells_written(&table, false) == expected,
            "case {case}: {:?}",
            table.merged_ranges
        );
    }
    assert!(merged > 1_000, "the inputs merge cells: {merged}");
}

#[test]
fn table_cells_take_linear_time() {
    // 100,000 cells, alone and with 50,000 merged ranges: the original searched the ranges
    // for every cell, about 2.5e9 comparisons (11 s in a debug build, against 0.25 s for the
    // cells alone).
    let plain = Table::new(Vector3::ZERO, 200, 500);
    let start = std::time::Instant::now();
    cells_written(&plain, false);
    let alone = start.elapsed();
    let mut table = plain;
    table.merged_ranges = (0..200)
        .flat_map(|r| {
            (0..500)
                .step_by(2)
                .map(move |c| CellRange::new(r, c, r, c + 1))
        })
        .collect();
    let start = std::time::Instant::now();
    let written = cells_written(&table, false);
    let elapsed = start.elapsed();
    let text = String::from_utf8_lossy(&written);
    let anchors = text.matches("175\r\n     2\r\n").count();
    assert_eq!(anchors, 50_000, "every merged pair spans two columns");
    assert!(
        elapsed < alone * 3 + std::time::Duration::from_secs(1),
        "{elapsed:?} against {alone:?} without merged ranges"
    );
}
