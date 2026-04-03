use std::path::Path;

use color_eyre::Result;
use ndarray::Array2;
use serde::Deserialize;

/// Load a .npy matrix file as an Array2<f64>.
/// Handles both f64 and f32 source data (converting f32 to f64).
pub fn load_npy_matrix(path: &Path) -> Result<Array2<f64>> {
    // Try f64 first
    let result: std::result::Result<Array2<f64>, _> = ndarray_npy::read_npy(path);
    match result {
        Ok(arr) => Ok(arr),
        Err(_) => {
            // Fall back to f32 and convert
            let arr_f32: Array2<f32> = ndarray_npy::read_npy(path)?;
            Ok(arr_f32.mapv(|v| v as f64))
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Timeseries {
    pub steps: Vec<f64>,
    pub values: Vec<f64>,
}

/// Load a timeseries from a JSON file with format: {"steps": [...], "values": [...]}
pub fn load_timeseries(path: &Path) -> Result<Timeseries> {
    let data = std::fs::read_to_string(path)?;
    let ts: Timeseries = serde_json::from_str(&data)?;
    Ok(ts)
}
