use super::super::transform;
use super::*;
use crate::types::Matrix3;
use std::f64::consts::{PI, TAU};

pub(crate) fn mirror_text(e: &mut Text, transform: &Transform) {
    mirror_text_with_options(e, transform, MirrorOptions::default());
}

pub(crate) fn mirror_text_with_options(
    e: &mut Text,
    transform: &Transform,
    options: MirrorOptions,
) {
    if !valid_transform(transform) {
        return;
    }
    let original_rotation = e.rotation;
    let width = options
        .text_width
        .filter(|w| w.is_finite() && *w >= 0.0)
        .unwrap_or_else(|| {
            crate::entities::text::estimated_width_in_heights(&e.value)
                * e.height
                * e.width_factor.abs()
        });
    let unused_alignment = if e.horizontal_alignment == TextHorizontalAlignment::Left
        && e.vertical_alignment == TextVerticalAlignment::Baseline
    {
        e.alignment_point.take()
    } else {
        None
    };
    transform::transform_text(e, transform);
    if transform::is_reflecting(transform) {
        if options.mirror_text {
            e.rotation = (e.rotation + PI).rem_euclid(TAU);
            e.generation_flags ^= 2;
        } else {
            let reverse_x = original_rotation.cos() * e.rotation.cos() < 0.0;
            if reverse_x {
                e.rotation += PI;
            }
            e.rotation = e.rotation.rem_euclid(TAU);
            let x = Vector3::new(e.rotation.cos(), e.rotation.sin(), 0.0);
            let y = Vector3::new(-x.y, x.x, 0.0);
            let sign_x = if e.generation_flags & 2 == 0 {
                1.0
            } else {
                -1.0
            };
            let sign_y = if e.generation_flags & 4 == 0 {
                1.0
            } else {
                -1.0
            };
            if reverse_x {
                if matches!(
                    e.horizontal_alignment,
                    TextHorizontalAlignment::Aligned | TextHorizontalAlignment::Fit
                ) {
                    if let Some(align) = &mut e.alignment_point {
                        std::mem::swap(&mut e.insertion_point, align);
                    }
                } else {
                    e.insertion_point = e.insertion_point - x * (width * sign_x);
                    if e.horizontal_alignment == TextHorizontalAlignment::Right {
                        if let Some(align) = &mut e.alignment_point {
                            *align = *align + x * (width * sign_x);
                        }
                    }
                }
            } else {
                let vertical = match e.vertical_alignment {
                    TextVerticalAlignment::Top => -1.0,
                    TextVerticalAlignment::Middle => 0.0,
                    _ => {
                        if e.horizontal_alignment == TextHorizontalAlignment::Middle {
                            0.0
                        } else {
                            1.0
                        }
                    }
                };
                let offset = y * (-e.height * sign_y * vertical);
                e.insertion_point = e.insertion_point + offset;
                if let Some(align) = &mut e.alignment_point {
                    *align = *align + offset;
                }
            }
        }
    }
    if unused_alignment.is_some() {
        e.alignment_point = unused_alignment;
    }
    e.common.raw_record = None;
}

pub(crate) fn mirror_mtext(e: &mut MText, transform: &Transform) {
    mirror_mtext_with_options(e, transform, MirrorOptions::default());
}

pub(crate) fn mirror_mtext_with_options(
    e: &mut MText,
    transform: &Transform,
    options: MirrorOptions,
) {
    if !valid_transform(transform) {
        return;
    }
    let old_x = Matrix3::arbitrary_axis(e.normal).transpose() * transform::mtext_x_direction(e);
    transform::transform_mtext(e, transform);
    if let Some(direction) = e.dwg_x_direction {
        e.rotation = direction.y.atan2(direction.x);
    }
    if transform::is_reflecting(transform) {
        if options.mirror_text {
            // MTEXT has no generation flags; the oriented normal carries
            // reflection of the glyph frame.
            e.normal = -e.normal;
        } else {
            let mut direction = e.dwg_x_direction.unwrap();
            let new_x = Matrix3::arbitrary_axis(e.normal).transpose() * direction;
            let reverse_x = old_x.x * new_x.x < 0.0;
            if reverse_x {
                direction = -direction;
            }
            e.dwg_x_direction = Some(direction);
            e.rotation = direction.y.atan2(direction.x);
            let attachment = e.attachment_point as i32 - 1;
            let (row, column) = (attachment / 3, attachment % 3);
            let (row, column) = if reverse_x {
                (row, 2 - column)
            } else {
                (2 - row, column)
            };
            e.attachment_point = match row * 3 + column + 1 {
                1 => AttachmentPoint::TopLeft,
                2 => AttachmentPoint::TopCenter,
                3 => AttachmentPoint::TopRight,
                4 => AttachmentPoint::MiddleLeft,
                5 => AttachmentPoint::MiddleCenter,
                6 => AttachmentPoint::MiddleRight,
                7 => AttachmentPoint::BottomLeft,
                8 => AttachmentPoint::BottomCenter,
                _ => AttachmentPoint::BottomRight,
            };
        }
    }
    e.common.raw_record = None;
}

fn attribute_text(
    value: &str,
    insertion: Vector3,
    alignment: Vector3,
    height: f64,
    rotation: f64,
    width_factor: f64,
    oblique: f64,
    normal: Vector3,
    flags: i16,
    horizontal: HorizontalAlignment,
    vertical: VerticalAlignment,
) -> Text {
    Text {
        value: value.to_owned(),
        insertion_point: insertion,
        alignment_point: Some(alignment),
        height,
        rotation,
        width_factor,
        oblique_angle: oblique,
        normal,
        generation_flags: flags,
        horizontal_alignment: match horizontal {
            HorizontalAlignment::Left => TextHorizontalAlignment::Left,
            HorizontalAlignment::Center => TextHorizontalAlignment::Center,
            HorizontalAlignment::Right => TextHorizontalAlignment::Right,
            HorizontalAlignment::Aligned => TextHorizontalAlignment::Aligned,
            HorizontalAlignment::Middle => TextHorizontalAlignment::Middle,
            HorizontalAlignment::Fit => TextHorizontalAlignment::Fit,
        },
        vertical_alignment: match vertical {
            VerticalAlignment::Baseline => TextVerticalAlignment::Baseline,
            VerticalAlignment::Bottom => TextVerticalAlignment::Bottom,
            VerticalAlignment::Middle => TextVerticalAlignment::Middle,
            VerticalAlignment::Top => TextVerticalAlignment::Top,
        },
        ..Text::new()
    }
}

macro_rules! mirror_attribute {
    ($name:ident, $with_options:ident, $ty:ty, $value:ident) => {
        pub(crate) fn $name(e: &mut $ty, transform: &Transform) {
            $with_options(e, transform, MirrorOptions::default());
        }
        pub(crate) fn $with_options(e: &mut $ty, transform: &Transform, options: MirrorOptions) {
            if !valid_transform(transform) {
                return;
            }
            let mut text = attribute_text(
                &e.$value,
                e.insertion_point,
                e.alignment_point,
                e.height,
                e.rotation,
                e.width_factor,
                e.oblique_angle,
                e.normal,
                e.text_generation_flags,
                e.horizontal_alignment,
                e.vertical_alignment,
            );
            if e.horizontal_alignment == HorizontalAlignment::Left
                && e.vertical_alignment == VerticalAlignment::Baseline
            {
                text.alignment_point = None;
            }
            mirror_text_with_options(&mut text, transform, options);
            e.insertion_point = text.insertion_point;
            if let Some(point) = text.alignment_point {
                e.alignment_point = point;
            }
            e.rotation = text.rotation;
            e.height = text.height;
            e.normal = text.normal;
            e.text_generation_flags = text.generation_flags;
            if let Some(mtext) = &mut e.embedded_mtext {
                mirror_mtext_with_options(mtext, transform, options);
            }
            e.common.raw_record = None;
        }
    };
}

mirror_attribute!(
    mirror_attribute_definition,
    mirror_attribute_definition_with_options,
    AttributeDefinition,
    default_value
);
mirror_attribute!(
    mirror_attribute_entity,
    mirror_attribute_entity_with_options,
    AttributeEntity,
    value
);

pub(crate) fn mirror_shape(e: &mut Shape, transform: &Transform) {
    if !valid_transform(transform) {
        return;
    }
    transform::transform_shape(e, transform);
    if transform::is_reflecting(transform) {
        e.rotation = (e.rotation + PI).rem_euclid(TAU);
        let cosine = e.oblique_angle.cos();
        if cosine.abs() > 1e-12 {
            e.size *= cosine.abs();
            e.relative_x_scale = -e.relative_x_scale.abs() / cosine.abs();
        } else {
            e.relative_x_scale = -e.relative_x_scale;
        }
        e.oblique_angle = -e.oblique_angle;
    }
    e.common.raw_record = None;
}
