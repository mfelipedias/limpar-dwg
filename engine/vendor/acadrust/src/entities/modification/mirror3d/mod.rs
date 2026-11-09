//! MIRROR3D reflects entities across a world-space plane.
//!
//! Planar MIRROR is exposed separately by [`super::mirror`]. Plane selectors
//! carry coordinates resolved by the caller, including object or view planes.

use super::mirror::MirrorOptions;
use crate::entities::EntityType;
use crate::types::{Transform, Vector3};

/// Plane selection for the MIRROR3D operation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MirrorPlane {
    PointNormal {
        point: Vector3,
        normal: Vector3,
    },
    ThreePoint {
        first: Vector3,
        second: Vector3,
        third: Vector3,
    },
    XY {
        point: Vector3,
    },
    YZ {
        point: Vector3,
    },
    ZX {
        point: Vector3,
    },
}

impl MirrorPlane {
    pub(crate) fn transform(self) -> Transform {
        let (point, normal) = match self {
            Self::PointNormal { point, normal } => (point, normal),
            Self::ThreePoint {
                first,
                second,
                third,
            } => {
                let direction = |end: Vector3| -> Option<Vector3> {
                    if [first.x, first.y, first.z, end.x, end.y, end.z]
                        .iter()
                        .any(|v| !v.is_finite())
                    {
                        return None;
                    }
                    let mut delta = end - first;
                    if [delta.x, delta.y, delta.z].iter().any(|v| !v.is_finite()) {
                        let scale = [first.x, first.y, first.z, end.x, end.y, end.z]
                            .iter()
                            .map(|v| v.abs())
                            .fold(0.0, f64::max);
                        delta = Vector3::new(
                            end.x / scale - first.x / scale,
                            end.y / scale - first.y / scale,
                            end.z / scale - first.z / scale,
                        );
                    }
                    let scale = delta.x.abs().max(delta.y.abs()).max(delta.z.abs());
                    (scale > 0.0)
                        .then(|| Vector3::new(delta.x / scale, delta.y / scale, delta.z / scale))
                };
                let (Some(a), Some(b)) = (direction(second), direction(third)) else {
                    return Transform::identity();
                };
                (first, a.cross(&b))
            }
            Self::XY { point } => (point, Vector3::UNIT_Z),
            Self::YZ { point } => (point, Vector3::UNIT_X),
            Self::ZX { point } => (point, Vector3::UNIT_Y),
        };
        Transform::from_mirror_plane(point, normal)
    }
}

impl EntityType {
    /// MIRROR3D across the selected plane, with readable text by default.
    pub fn mirror3d(&mut self, plane: MirrorPlane) {
        self.mirror3d_with_options(plane, MirrorOptions::default());
    }

    /// MIRROR3D with an explicit MIRRTEXT policy.
    pub fn mirror3d_with_options(&mut self, plane: MirrorPlane, options: MirrorOptions) {
        self.apply_mirror_with_options(&plane.transform(), options);
    }
}

#[cfg(test)]
mod tests;
