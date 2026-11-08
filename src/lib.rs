//! Estimate how many billable Datadog custom metrics a sample produces.
//!
//! Offline. The library reads samples the caller already has, and an optional
//! price file. It does not open a network connection.

pub mod advice;
pub mod allow;
pub mod cost;
pub mod count;
pub mod diff;
pub mod parse;
pub mod render;
mod series;
pub mod tags;
