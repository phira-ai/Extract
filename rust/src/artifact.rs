use std::path::Path;

use color_eyre::Result;
use ndarray::Array2;

/// A single cell value in a table, supporting multiple numeric types.
#[derive(Debug, Clone)]
pub enum CellValue {
    Float(f64),
    Int(i64),
}

impl CellValue {
    /// Format the cell for display with a given width.
    pub fn display(&self, width: usize) -> String {
        match self {
            CellValue::Float(v) if v.is_nan() => format!("{:>width$}", "\u{00b7}"),
            CellValue::Float(v) => format!("{:>width$.2}", v),
            CellValue::Int(v) => format!("{:>width$}", v),
        }
    }

    /// Get the numeric value as f64 for highlight rule matching.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            CellValue::Float(v) if v.is_nan() => None,
            CellValue::Float(v) => Some(*v),
            CellValue::Int(v) => Some(*v as f64),
        }
    }
}

/// A loaded table with rows x cols of cell values.
#[derive(Debug, Clone)]
pub struct TableData {
    pub rows: usize,
    pub cols: usize,
    pub values: Vec<Vec<CellValue>>,
}

/// Load a .npy file as a TableData, trying multiple dtypes.
pub fn load_table(path: &Path) -> Result<TableData> {
    // Try f64
    let r: std::result::Result<Array2<f64>, _> = ndarray_npy::read_npy(path);
    if let Ok(arr) = r {
        return Ok(f64_array_to_table(&arr));
    }
    // Try f32 → f64
    let r: std::result::Result<Array2<f32>, _> = ndarray_npy::read_npy(path);
    if let Ok(arr) = r {
        return Ok(f64_array_to_table(&arr.mapv(|v| v as f64)));
    }
    // Try i64
    let r: std::result::Result<Array2<i64>, _> = ndarray_npy::read_npy(path);
    if let Ok(arr) = r {
        return Ok(i64_array_to_table(&arr));
    }
    // Try i32 → i64
    let r: std::result::Result<Array2<i32>, _> = ndarray_npy::read_npy(path);
    if let Ok(arr) = r {
        return Ok(i64_array_to_table(&arr.mapv(|v| v as i64)));
    }
    color_eyre::eyre::bail!("Unsupported numpy dtype in {}", path.display())
}

fn f64_array_to_table(arr: &Array2<f64>) -> TableData {
    let (rows, cols) = arr.dim();
    let values = (0..rows)
        .map(|r| {
            (0..cols)
                .map(|c| CellValue::Float(arr[[r, c]]))
                .collect()
        })
        .collect();
    TableData { rows, cols, values }
}

fn i64_array_to_table(arr: &Array2<i64>) -> TableData {
    let (rows, cols) = arr.dim();
    let values = (0..rows)
        .map(|r| {
            (0..cols)
                .map(|c| CellValue::Int(arr[[r, c]]))
                .collect()
        })
        .collect();
    TableData { rows, cols, values }
}

