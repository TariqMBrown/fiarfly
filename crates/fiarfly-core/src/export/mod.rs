//! Export — write results to Parquet and CSV via Polars.
//!
//! Implementation spec: IMPLEMENTATION_GUIDE.md §5

mod dataframe;
pub use dataframe::{to_dataframe, write_csv, write_parquet, ExportData};
