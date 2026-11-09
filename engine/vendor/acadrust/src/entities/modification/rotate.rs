//! Rotation tools for CAD entities.
//!
//! Angles are in radians and positive rotations follow the right-hand rule.
//! Opaque proxy/unknown records and nongeometric sequence markers are preserved
//! because their payload does not expose geometry that can be transformed.

use crate::entities::EntityType;
use crate::types::{Transform, Vector3};

/// Axis choices corresponding to the 3D CAD rotation workflow.
///
/// `Entity`, `Last`, and `View` carry the axis line resolved by the caller;
/// the library does not own an interactive selection or active-view state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RotationAxis {
    Entity { origin: Vector3, direction: Vector3 },
    Last { origin: Vector3, direction: Vector3 },
    View { origin: Vector3, direction: Vector3 },
    XAxis { origin: Vector3 },
    YAxis { origin: Vector3 },
    ZAxis { origin: Vector3 },
    TwoPoint { first: Vector3, second: Vector3 },
}

impl RotationAxis {
    fn line(self) -> Option<(Vector3, Vector3)> {
        match self {
            Self::Entity { origin, direction }
            | Self::Last { origin, direction }
            | Self::View { origin, direction } => Some((origin, direction)),
            Self::XAxis { origin } => Some((origin, Vector3::UNIT_X)),
            Self::YAxis { origin } => Some((origin, Vector3::UNIT_Y)),
            Self::ZAxis { origin } => Some((origin, Vector3::UNIT_Z)),
            Self::TwoPoint { first, second } => Some((first, second - first)),
        }
    }
}

/// Return the signed angle from `reference` to `target` around `axis`, after
/// projecting both vectors into the plane perpendicular to the axis.
pub(crate) fn angle_from_points(
    center: Vector3,
    axis: Vector3,
    reference: Vector3,
    target: Vector3,
) -> Option<f64> {
    let finite = |v: Vector3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
    if !finite(center) || !finite(axis) || !finite(reference) || !finite(target) {
        return None;
    }
    let axis = axis.normalize();
    if axis.length_squared() <= f64::EPSILON {
        return None;
    }
    let project = |point: Vector3| {
        let vector = point - center;
        vector - axis * vector.dot(&axis)
    };
    let reference = project(reference);
    let target = project(target);
    if reference.length_squared() <= f64::EPSILON || target.length_squared() <= f64::EPSILON {
        return None;
    }
    Some(
        axis.dot(&reference.cross(&target))
            .atan2(reference.dot(&target)),
    )
}

impl EntityType {
    /// Rotate about `center` in the world XY plane.
    pub fn rotate(&mut self, center: Vector3, angle: f64) {
        self.rotate_about_axis(center, Vector3::UNIT_Z, angle);
    }

    /// Rotate about the world-space line through `center` parallel to `axis`.
    ///
    /// A zero-length axis or a zero angle leaves the entity unchanged. Unknown
    /// and opaque application-defined geometry remains preserved verbatim.
    pub fn rotate_about_axis(&mut self, center: Vector3, axis: Vector3, angle: f64) {
        if !center.x.is_finite()
            || !center.y.is_finite()
            || !center.z.is_finite()
            || !axis.x.is_finite()
            || !axis.y.is_finite()
            || !axis.z.is_finite()
            || !angle.is_finite()
            || axis.length_squared() <= f64::EPSILON
            || angle == 0.0
        {
            return;
        }
        let transform = Transform::from_rotation_with_origin(axis, angle, center);
        self.apply_transform(&transform);
    }

    /// Rotate using one of the CAD axis-selection modes.
    pub fn rotate3d(&mut self, axis: RotationAxis, angle: f64) {
        self.rotate_with_axis(axis, angle);
    }

    /// Rotate using one of the CAD axis-selection modes.
    pub fn rotate_with_axis(&mut self, axis: RotationAxis, angle: f64) {
        match axis {
            RotationAxis::TwoPoint { first, second } => {
                self.rotate_about_line(first, second, angle)
            }
            _ => {
                let Some((origin, direction)) = axis.line() else {
                    return;
                };
                self.rotate_about_axis(origin, direction, angle);
            }
        }
    }

    /// Alias for [`Self::rotate_with_axis`].
    pub fn rotate_axis(&mut self, axis: RotationAxis, angle: f64) {
        self.rotate_with_axis(axis, angle);
    }

    /// Rotate around the 3D line passing through `first` and `second`.
    pub fn rotate_about_line(&mut self, first: Vector3, second: Vector3, angle: f64) {
        let axis = second - first;
        if !first.x.is_finite()
            || !first.y.is_finite()
            || !first.z.is_finite()
            || !second.x.is_finite()
            || !second.y.is_finite()
            || !second.z.is_finite()
            || !angle.is_finite()
            || axis.length_squared() <= f64::EPSILON
        {
            return;
        }
        self.rotate_about_axis(first, axis, angle);
    }

    /// Rotate from a reference point to a target point around `center` in the
    /// world XY plane. The two points define the signed rotation angle.
    pub fn rotate_by_points(&mut self, center: Vector3, reference: Vector3, target: Vector3) {
        self.rotate_about_axis_by_points(center, Vector3::UNIT_Z, reference, target);
    }

    /// Alias for [`Self::rotate_by_points`].
    pub fn rotate_from_points(&mut self, center: Vector3, reference: Vector3, target: Vector3) {
        self.rotate_by_points(center, reference, target);
    }

    /// Rotate from a reference point to a target point around an arbitrary
    /// axis through `center`.
    pub fn rotate_about_axis_by_points(
        &mut self,
        center: Vector3,
        axis: Vector3,
        reference: Vector3,
        target: Vector3,
    ) {
        let Some(angle) = angle_from_points(center, axis, reference, target) else {
            return;
        };
        self.rotate_about_axis(center, axis, angle);
    }

    /// Alias for [`Self::rotate_about_axis_by_points`].
    pub fn rotate_about_axis_from_points(
        &mut self,
        center: Vector3,
        axis: Vector3,
        reference: Vector3,
        target: Vector3,
    ) {
        self.rotate_about_axis_by_points(center, axis, reference, target);
    }

    /// Apply rotation around `axis` through the world-space `origin` point.
    pub fn apply_rotation_with_origin(&mut self, axis: Vector3, angle: f64, origin: Vector3) {
        self.rotate_about_axis(origin, axis, angle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{
        Entity, Light, LightPhotometricData, Line, MText, SectionSymbol, SectionSymbolPoint,
        UnknownEntity, ViewBorder,
    };
    use std::f64::consts::FRAC_PI_2;

    fn assert_vector(actual: Vector3, expected: Vector3) {
        assert!(
            actual.distance(&expected) < 1e-9,
            "expected {expected:?}, got {actual:?}"
        );
    }

    #[test]
    fn rotation_moves_light_position_and_target() {
        let mut light = Light::new();
        light.position = Vector3::new(3.0, 2.0, 1.0);
        light.target = Vector3::new(4.0, 2.0, 1.0);
        let mut photometric = LightPhotometricData::default();
        photometric.web_rotation = Vector3::UNIT_X;
        light.photometric_data = Some(photometric);
        let mut entity = EntityType::Light(Box::new(light));
        entity.rotate(Vector3::new(2.0, 2.0, 0.0), FRAC_PI_2);
        let EntityType::Light(light) = entity else {
            unreachable!()
        };
        assert_vector(light.position, Vector3::new(2.0, 3.0, 1.0));
        assert_vector(light.target, Vector3::new(2.0, 4.0, 1.0));
        assert_vector(
            light.photometric_data.unwrap().web_rotation,
            Vector3::UNIT_Y,
        );
    }

    #[test]
    fn rotation_moves_section_points_and_label_offsets() {
        let mut section = SectionSymbol::new();
        section.points.push(SectionSymbolPoint {
            point: Vector3::new(2.0, 0.0, 0.0),
            label_offset: Vector3::UNIT_Y,
            ..SectionSymbolPoint::new()
        });
        section.sync_display_fields();
        let mut entity = EntityType::SectionSymbol(section);
        entity.rotate(Vector3::ZERO, FRAC_PI_2);
        let EntityType::SectionSymbol(section) = entity else {
            unreachable!()
        };
        assert_vector(section.points[0].point, Vector3::new(0.0, 2.0, 0.0));
        assert_vector(section.points[0].label_offset, -Vector3::UNIT_X);
        assert!((section.tick_a - section.points[0].label_offset.y).abs() < 1e-9);
    }

    #[test]
    fn rotation_updates_view_border_orientation() {
        let mut border = ViewBorder::new();
        border.min = [1.0, 0.0];
        border.max = [3.0, 2.0];
        border.center = [2.0, 1.0];
        let mut entity = EntityType::ViewBorder(border);
        entity.rotate(Vector3::ZERO, FRAC_PI_2);
        let EntityType::ViewBorder(border) = entity else {
            unreachable!()
        };
        assert_vector(
            Vector3::new(border.center[0], border.center[1], 0.0),
            Vector3::new(-1.0, 2.0, 0.0),
        );
        assert!((border.rotation_angle - FRAC_PI_2).abs() < 1e-9);
    }

    #[test]
    fn rotation_preserves_unknown_payload() {
        let mut unknown = UnknownEntity::new("CUSTOM_GEOMETRY");
        unknown.raw_dwg_data = Some(vec![1, 2, 3]);
        let mut entity = EntityType::Unknown(unknown);
        let original = entity.clone();
        entity.rotate_about_axis(Vector3::new(1.0, 2.0, 3.0), Vector3::UNIT_X, FRAC_PI_2);
        assert_eq!(entity, original);
    }

    #[test]
    fn entity_trait_rotation_uses_world_xy_pivot() {
        let mut line =
            Line::from_points(Vector3::new(10.0, 0.0, 0.0), Vector3::new(20.0, 0.0, 0.0));
        line.rotate(Vector3::new(10.0, 10.0, 0.0), FRAC_PI_2);
        assert_vector(line.start, Vector3::new(20.0, 10.0, 0.0));
        assert_vector(line.end, Vector3::new(20.0, 20.0, 0.0));
    }

    #[test]
    fn rotate3d_preserves_mtext_direction_out_of_xy_plane() {
        let mut mtext = MText::with_value("tilted", Vector3::ZERO);
        mtext.rotation = std::f64::consts::FRAC_PI_2;
        let mut entity = EntityType::MText(Box::new(mtext));
        entity.rotate3d(
            RotationAxis::XAxis {
                origin: Vector3::ZERO,
            },
            FRAC_PI_2,
        );
        let EntityType::MText(mtext) = entity else {
            unreachable!()
        };
        assert_vector(mtext.normal, Vector3::new(0.0, -1.0, 0.0));
        assert_vector(
            mtext.dwg_x_direction.unwrap_or(Vector3::ZERO),
            Vector3::UNIT_Z,
        );
        assert!(mtext.rotation.abs() < 1e-12, "rotation={}", mtext.rotation);
    }

    #[test]
    fn rotation_from_two_points_derives_signed_xy_angle() {
        let mut line = EntityType::Line(Line::from_points(
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(2.0, 0.0, 0.0),
        ));
        line.rotate_by_points(Vector3::ZERO, Vector3::UNIT_X, Vector3::UNIT_Y);
        let EntityType::Line(line) = line else {
            unreachable!()
        };
        assert_vector(line.start, Vector3::new(0.0, 1.0, 0.0));
        assert_vector(line.end, Vector3::new(0.0, 2.0, 0.0));
    }

    #[test]
    fn rotation_from_two_points_supports_arbitrary_axis() {
        let mut line = EntityType::Line(Line::from_points(
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 2.0, 0.0),
        ));
        line.rotate_about_axis_by_points(
            Vector3::ZERO,
            Vector3::UNIT_X,
            Vector3::UNIT_Y,
            Vector3::UNIT_Z,
        );
        let EntityType::Line(line) = line else {
            unreachable!()
        };
        assert_vector(line.start, Vector3::new(0.0, 0.0, 1.0));
        assert_vector(line.end, Vector3::new(0.0, 0.0, 2.0));
    }

    #[test]
    fn rotation_about_two_point_axis() {
        let mut line = EntityType::Line(Line::from_points(
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 2.0, 0.0),
        ));
        line.rotate_about_line(Vector3::ZERO, Vector3::UNIT_X, FRAC_PI_2);
        let EntityType::Line(line) = line else {
            unreachable!()
        };
        assert_vector(line.start, Vector3::new(0.0, 0.0, 1.0));
        assert_vector(line.end, Vector3::new(0.0, 0.0, 2.0));
    }

    #[test]
    fn rotation_axis_selector_supports_two_point_and_world_axes() {
        let mut line = EntityType::Line(Line::from_points(
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 2.0, 0.0),
        ));
        line.rotate3d(
            RotationAxis::XAxis {
                origin: Vector3::ZERO,
            },
            FRAC_PI_2,
        );
        let EntityType::Line(line) = line else {
            unreachable!()
        };
        assert_vector(line.start, Vector3::new(0.0, 0.0, 1.0));

        let mut line = EntityType::Line(Line::from_points(
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(2.0, 0.0, 0.0),
        ));
        line.rotate3d(
            RotationAxis::TwoPoint {
                first: Vector3::ZERO,
                second: Vector3::UNIT_Z,
            },
            FRAC_PI_2,
        );
        let EntityType::Line(line) = line else {
            unreachable!()
        };
        assert_vector(line.start, Vector3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn nonfinite_two_point_rotation_is_a_noop() {
        let mut line = EntityType::Line(Line::from_points(Vector3::UNIT_X, Vector3::UNIT_Y));
        let original = line.clone();
        line.rotate_by_points(
            Vector3::ZERO,
            Vector3::new(f64::NAN, 0.0, 0.0),
            Vector3::UNIT_Y,
        );
        assert_eq!(line, original);
    }

    #[test]
    fn entity_type_rotation_invalidates_verbatim_dwg_provenance() {
        use crate::entities::{EntityType, RawRecord};
        use crate::types::DxfVersion;
        use std::sync::Arc;

        let mut line = EntityType::Line(Line::from_points(Vector3::ZERO, Vector3::UNIT_X));
        line.common_mut().raw_record = Some(Arc::new(RawRecord {
            data: vec![1, 2, 3],
            handle_bits: 0,
            version: DxfVersion::AC1024,
        }));
        line.rotate3d(
            RotationAxis::ZAxis {
                origin: Vector3::ZERO,
            },
            FRAC_PI_2,
        );
        assert!(line.common().raw_record.is_none());
    }
}
