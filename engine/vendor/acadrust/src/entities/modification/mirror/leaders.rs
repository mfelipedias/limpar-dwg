use super::super::transform;
use super::*;

pub(crate) fn mirror_leader(e: &mut Leader, transform: &Transform) {
    if !valid_transform(transform) {
        return;
    }
    let block_offset = transform.apply_rotation(e.block_offset);
    let annotation_offset = transform.apply_rotation(e.annotation_offset);
    let origin = transform.apply(e.origin);
    transform::transform_leader(e, transform);
    e.block_offset = block_offset;
    e.annotation_offset = annotation_offset;
    e.origin = origin;
    e.common.raw_record = None;
}
