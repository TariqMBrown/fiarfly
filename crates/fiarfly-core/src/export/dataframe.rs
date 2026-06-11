//! Polars DataFrame construction and file writing.

use ndarray::Array2;
use polars::prelude::{DataFrame, Series, NamedFrom, ParquetWriter, CsvWriter, SerWriter};
use crate::{FiarflyError, roi::RoiSet};
use crate::signal::OasisResult;

pub struct ExportData<'a> {
    pub roi_set: &'a RoiSet,
    /// Shape: [n_rois, n_frames]
    pub raw_f: &'a Array2<f32>,
    /// Shape: [n_rois, n_frames]
    pub delta_f: &'a Array2<f32>,
    /// Neuropil fluorescence (optional). Shape: [n_rois, n_frames]
    pub neuropil_f: Option<&'a Array2<f32>>,
    pub deconvolved: Option<&'a OasisResult>,
    pub frame_rate: Option<f32>,
    pub session_id: &'a str,
}

/// Build a tidy long-format Polars DataFrame.
///
/// One row per (ROI × frame).  Optional columns are included only when
/// the corresponding data is provided.
pub fn to_dataframe(data: &ExportData) -> Result<DataFrame, FiarflyError> {
    let (n_rois, n_frames) = data.raw_f.dim();
    let total = n_rois * n_frames;

    let mut roi_ids      = Vec::with_capacity(total);
    let mut roi_labels   = Vec::with_capacity(total);
    let mut roi_groups:  Vec<Option<String>> = Vec::with_capacity(total);
    let mut session_ids  = Vec::with_capacity(total);
    let mut frame_idxs:  Vec<u32>  = Vec::with_capacity(total);
    let mut time_s:      Vec<Option<f32>> = Vec::with_capacity(total);
    let mut raw_f_vals:  Vec<f32>  = Vec::with_capacity(total);
    let mut delta_f_vals: Vec<f32> = Vec::with_capacity(total);

    for (i, roi) in data.roi_set.rois.iter().enumerate() {
        for t in 0..n_frames {
            roi_ids.push(roi.id.clone());
            roi_labels.push(roi.label.clone());
            roi_groups.push(roi.group.clone());
            session_ids.push(data.session_id.to_owned());
            frame_idxs.push(t as u32);
            time_s.push(data.frame_rate.map(|fr| t as f32 / fr));
            raw_f_vals.push(if i < n_rois { data.raw_f[[i, t]] } else { f32::NAN });
            delta_f_vals.push(if i < n_rois { data.delta_f[[i, t]] } else { f32::NAN });
        }
    }

    let mut series: Vec<Series> = vec![
        Series::new("roi_id".into(),        roi_ids),
        Series::new("roi_label".into(),      roi_labels),
        Series::new("session_id".into(),     session_ids),
        Series::new("frame_idx".into(),      frame_idxs),
        Series::new("raw_f".into(),          raw_f_vals),
        Series::new("delta_f_over_f".into(), delta_f_vals),
    ];

    // Nullable roi_group (Option<String>).
    let group_series = Series::new("roi_group".into(),
        roi_groups.iter().map(|g| g.as_deref()).collect::<Vec<Option<&str>>>());
    series.push(group_series);

    // Nullable time_s (Option<f32>).
    let time_series = Series::new("time_s".into(), time_s);
    series.push(time_series);

    // Optional neuropil column.
    if let Some(np) = data.neuropil_f {
        let vals: Vec<f32> = (0..n_rois)
            .flat_map(|i| (0..n_frames).map(move |t| np[[i, t]]))
            .collect();
        series.push(Series::new("neuropil_f".into(), vals));
    }

    // Optional deconvolved spikes column.
    if let Some(oasis) = data.deconvolved {
        let vals: Vec<f32> = (0..n_rois)
            .flat_map(|i| (0..n_frames).map(move |t| {
                if i < oasis.spikes.nrows() && t < oasis.spikes.ncols() {
                    oasis.spikes[[i, t]]
                } else {
                    f32::NAN
                }
            }))
            .collect();
        series.push(Series::new("deconvolved".into(), vals));
    }

    DataFrame::new(series).map_err(|e| FiarflyError::Export(e.to_string()))
}

pub fn write_parquet(df: &mut DataFrame, path: &std::path::Path) -> Result<(), FiarflyError> {
    let file = std::fs::File::create(path)?;
    ParquetWriter::new(file)
        .finish(df)
        .map_err(|e| FiarflyError::Export(e.to_string()))?;
    Ok(())
}

pub fn write_csv(df: &mut DataFrame, path: &std::path::Path) -> Result<(), FiarflyError> {
    let file = std::fs::File::create(path)?;
    CsvWriter::new(file)
        .finish(df)
        .map_err(|e| FiarflyError::Export(e.to_string()))?;
    Ok(())
}
