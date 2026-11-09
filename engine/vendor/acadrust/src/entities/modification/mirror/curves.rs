use super::super::transform;
use super::*;

pub(crate) fn mirror_spline(e: &mut Spline, transform: &Transform) {
    if !valid_transform(transform) {
        return;
    }
    transform::transform_spline(e, transform);
    e.common.raw_record = None;
}

pub(crate) fn mirror_helix(e: &mut Helix, transform: &Transform) {
    if !valid_transform(transform) {
        return;
    }
    transform::transform_helix(e, transform);
    e.common.raw_record = None;
}
