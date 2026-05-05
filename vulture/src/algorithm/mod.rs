//! Private algorithm internals: per-round helpers, the per-call
//! driver, and the rRAPTOR range scan. Nothing in this module is part
//! of the public API; downstream callers use [`crate::Query`],
//! [`crate::Timetable`], and [`crate::RaptorCache`].

pub(crate) mod boarding;
pub(crate) mod footpaths;
pub(crate) mod label_bag;
pub(crate) mod per_call;
pub(crate) mod range;
