// CRT scanlines overlay for Iron Curtain video playback.
//
// Mimics the OpenRA VideoPlayerWidget scanline overlay:
// a semi-transparent black sprite composited over the video at screen/render
// resolution so the dark rows are one display-pixel wide regardless of how
// much the source VQA frame has been upscaled.
//
// params.x = half_row_height in physical screen pixels
//            = round(logical_rendered_height / vqa_source_height * dpi_scale / 2)
// params.y/z/w = unused (Vec4 for 16-byte uniform alignment).

#import bevy_ui::ui_vertex_output::UiVertexOutput

struct ScanlinesMaterial {
    params: vec4<f32>,
};

@group(1) @binding(0)
var<uniform> material: ScanlinesMaterial;

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let half_row_height = material.params.x;
    // in.position.y is the fragment Y coordinate in physical screen pixels.
    let row = floor(in.position.y / half_row_height);
    // Darken every other row with 50% black — matches OpenRA's alpha = 128.
    if (row % 2.0 > 0.5) {
        return vec4<f32>(0.0, 0.0, 0.0, 128.0 / 255.0);
    }
    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
}
