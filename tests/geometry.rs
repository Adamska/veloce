use veloce::gpu::{AspectMode, CropPreset, Geometry};

fn close(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5
}

#[test]
fn letterboxes_wide_content_in_a_tall_viewport() {
    let placement = Geometry::default().placement(16.0 / 9.0, [1000.0, 1000.0]);
    assert!(close(placement.scale, [1.0, 9.0 / 16.0]));
    assert!(close(placement.source_uv([0.0, 0.0]), [0.0, 0.0]));
    assert!(close(placement.source_uv([1.0, 1.0]), [1.0, 1.0]));
}

#[test]
fn pillarboxes_classic_content_in_a_wide_viewport() {
    let placement = Geometry::default().placement(4.0 / 3.0, [1920.0, 1080.0]);
    assert!(close(placement.scale, [0.75, 1.0]));
}

#[test]
fn clockwise_rotation_maps_top_left_to_source_bottom_left() {
    let geometry = Geometry {
        quarter_turns: 1,
        ..Geometry::default()
    };
    let placement = geometry.placement(16.0 / 9.0, [1600.0, 900.0]);
    assert!(close(placement.source_uv([0.0, 0.0]), [0.0, 1.0]));
    assert!(close(placement.source_uv([1.0, 0.0]), [0.0, 0.0]));
    assert!(placement.scale[0] < placement.scale[1]);
}

#[test]
fn mirror_flips_the_horizontal_axis_only() {
    let geometry = Geometry {
        mirror: true,
        ..Geometry::default()
    };
    let placement = geometry.placement(1.0, [100.0, 100.0]);
    assert!(close(placement.source_uv([0.0, 0.0]), [1.0, 0.0]));
    assert!(close(placement.source_uv([1.0, 1.0]), [0.0, 1.0]));
}

#[test]
fn scope_crop_trims_rows_and_changes_the_display_aspect() {
    let geometry = Geometry {
        crop: CropPreset::Scope,
        ..Geometry::default()
    };
    let placement = geometry.placement(16.0 / 9.0, [1920.0, 1080.0]);
    let top = placement.source_uv([0.0, 0.0]);
    let bottom = placement.source_uv([1.0, 1.0]);
    assert!(top[1] > 0.1 && bottom[1] < 0.9);
    assert!((bottom[1] - top[1] - (16.0 / 9.0) / 2.39).abs() < 1e-4);
    assert!(close(placement.scale, [1.0, (16.0 / 9.0) / 2.39]));
}

#[test]
fn aspect_override_stretches_without_cropping() {
    let geometry = Geometry {
        aspect: AspectMode::Square,
        ..Geometry::default()
    };
    let placement = geometry.placement(16.0 / 9.0, [1600.0, 900.0]);
    assert!(close(placement.scale, [900.0 / 1600.0, 1.0]));
    assert!(close(placement.source_uv([1.0, 1.0]), [1.0, 1.0]));
}
