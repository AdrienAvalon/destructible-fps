//! Bounded canonical convex geometry for native inspection, not replicated gameplay.
//!
//! Meshes, point rays and translating box contacts share these exact quantized shapes.
//! No implementation of `StaticGeometry`: callers must use the explicit combined scene API,
//! so existing refined-world queries cannot silently discard the oblique solids.

pub mod fixture;
mod mesh;
mod query;
mod scene;
mod shape;

pub use query::{
    ConvexChord, ConvexQueryBudget, ConvexQueryLimits, ConvexQueryStats, MAX_CONVEX_QUERY_AXES,
    MAX_CONVEX_QUERY_FRAGMENTS, MAX_CONVEX_QUERY_PROJECTIONS,
};
pub use scene::{InspectionGeometry, SceneChord, SceneOwner, SceneTrace};
pub use shape::{ConvexFace, ConvexFaceInput, ConvexFragment, ConvexPlane};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConvexError {
    InvalidShape,
    Bounds,
    Budget,
    Arithmetic,
    InitialOverlap,
    Overlap,
    WorldQuery(crate::world::query::GeometryQueryError),
    WorldTrace(crate::world::query::ray::TraceError),
}

impl core::fmt::Display for ConvexError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "convex inspection geometry refused: {self:?}")
    }
}
impl std::error::Error for ConvexError {}
