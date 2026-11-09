//! Handle-based MIRROR operations using the document's MIRRTEXT setting.

use super::CadDocument;
use crate::entities::{mirror::valid_transform, MirrorOptions, MirrorPlane};
use crate::types::{Handle, Transform, Vector3};

impl CadDocument {
    /// Mirror an existing entity in place across a line in the world XY plane.
    /// Uses the drawing's MIRRTEXT setting. Returns false for a missing handle,
    /// unsupported geometry, or invalid line; handle and block membership persist.
    pub fn mirror_entity(&mut self, handle: Handle, first: Vector3, second: Vector3) -> bool {
        let options = MirrorOptions {
            mirror_text: self.header.mirror_text,
            ..MirrorOptions::default()
        };
        self.mirror_entity_with_options(handle, first, second, options)
    }

    /// Mirror in place with explicit policies, including an exact text width
    /// when font metrics are available to the caller.
    pub fn mirror_entity_with_options(
        &mut self,
        handle: Handle,
        first: Vector3,
        second: Vector3,
        options: MirrorOptions,
    ) -> bool {
        self.mirror_entity_with_transform(
            handle,
            Transform::from_mirror_line(first, second),
            options,
        )
    }

    /// MIRROR3D an existing entity in place using the drawing's MIRRTEXT setting.
    /// Returns false for a missing handle, unsupported geometry, or invalid plane.
    pub fn mirror3d_entity(&mut self, handle: Handle, plane: MirrorPlane) -> bool {
        let options = MirrorOptions {
            mirror_text: self.header.mirror_text,
            ..MirrorOptions::default()
        };
        self.mirror3d_entity_with_options(handle, plane, options)
    }

    /// MIRROR3D in place with explicit text and hatch policies.
    pub fn mirror3d_entity_with_options(
        &mut self,
        handle: Handle,
        plane: MirrorPlane,
        options: MirrorOptions,
    ) -> bool {
        self.mirror_entity_with_transform(handle, plane.transform(), options)
    }

    fn mirror_entity_with_transform(
        &mut self,
        handle: Handle,
        transform: Transform,
        options: MirrorOptions,
    ) -> bool {
        if !valid_transform(&transform) {
            return false;
        }
        let Some(source) = self.get_entity(handle) else {
            return false;
        };
        if !crate::entities::transform::transform_invalidates_raw_record(source) {
            return false;
        }
        let Some(entity) = self.get_entity_mut(handle) else {
            return false;
        };
        entity.apply_mirror_with_options(&transform, options);
        true
    }
}
