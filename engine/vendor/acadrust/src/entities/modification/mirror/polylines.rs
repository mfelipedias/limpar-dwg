use super::super::transform;
use super::*;

pub(crate) fn mirror_mline(e: &mut MLine, transform: &Transform) {
    if !valid_transform(transform) {
        return;
    }
    transform::transform_mline(e, transform);
    if transform::is_reflecting(transform) {
        e.scale_factor = -e.scale_factor;
    }
    e.common.raw_record = None;
}
