use super::super::transform;
use super::*;

pub(crate) fn mirror_arc(e: &mut Arc, transform: &Transform) {
    if !valid_transform(transform) {
        return;
    }
    // The general transform already recomputes endpoint angles in OCS and
    // swaps them under reflection to retain the counterclockwise sweep.
    transform::transform_arc(e, transform);
    e.common.raw_record = None;
}

pub(crate) fn mirror_ellipse(e: &mut Ellipse, transform: &Transform) {
    if !super::valid_transform(transform) {
        return;
    }
    // The transformed axes already encode reflection: the new normal is
    // their cross product, so parameters stay attached to the same points.
    transform::transform_ellipse(e, transform);
    e.common.raw_record = None;
}

pub(crate) fn mirror_lwpolyline(e: &mut LwPolyline, transform: &Transform) {
    if !valid_transform(transform) {
        return;
    }
    // Bulge reversal is already performed by the general transform.
    transform::transform_lwpolyline(e, transform);
    e.common.raw_record = None;
}

pub(crate) fn mirror_polyline2d(e: &mut Polyline2D, transform: &Transform) {
    if !valid_transform(transform) {
        return;
    }
    transform::transform_polyline2d(e, transform);
    e.common.raw_record = None;
}
