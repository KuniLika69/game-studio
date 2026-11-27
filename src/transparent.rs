use studio_core::render_settings::BlendMode;

pub(crate) fn transparent_blend(mode: BlendMode) -> wgpu::BlendState {
    use wgpu::BlendFactor::{Dst, One, OneMinusSrc, OneMinusSrcAlpha, SrcAlpha};
    let (src_factor, dst_factor) = match mode {
        BlendMode::Normal => (SrcAlpha, OneMinusSrcAlpha),
        BlendMode::Add => (SrcAlpha, One),
        BlendMode::Multiply => (Dst, OneMinusSrcAlpha),
        BlendMode::Screen => (One, OneMinusSrc),
    };
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor,
            dst_factor,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendState::ALPHA_BLENDING.alpha,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TransparentLayer {
    Model { model: usize, primitive: usize },
    Water(usize),
    Particle(usize),
}

pub(super) struct LayerDepth {
    pub(super) layer: TransparentLayer,
    pub(super) distance_squared: f32,
    pub(super) decal: bool,
}

pub(super) fn sort_layers(layers: &mut [LayerDepth]) {
    layers.sort_by(|left, right| {
        right
            .distance_squared
            .total_cmp(&left.distance_squared)
            .then_with(|| left.decal.cmp(&right.decal))
    });
}

pub(crate) struct SortSample {
    pub(crate) ms: f32,
    pub(crate) order: Vec<TransparentLayer>,
}

#[derive(Default)]
pub(crate) struct TransparencyProbe(pub(crate) Vec<SortSample>);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a4_transparent_sort_is_far_first_with_facial_decals_after_equal_depth() {
        let layer = |index, depth, decal| LayerDepth {
            layer: TransparentLayer::Water(index),
            distance_squared: depth,
            decal,
        };
        let mut layers = [
            layer(0, 3.0, true),
            layer(1, 9.0, false),
            layer(2, 3.0, false),
        ];
        sort_layers(&mut layers);
        assert_eq!(
            layers.map(|entry| entry.layer),
            [
                TransparentLayer::Water(1),
                TransparentLayer::Water(2),
                TransparentLayer::Water(0)
            ]
        );
    }

    #[test]
    fn a4_blend_pipeline_uses_the_factors_for_each_equation() {
        use wgpu::BlendFactor::{Dst, One, OneMinusSrc, OneMinusSrcAlpha, SrcAlpha};
        assert_eq!(
            BlendMode::ALL.map(|mode| {
                let color = transparent_blend(mode).color;
                (color.src_factor, color.dst_factor)
            }),
            [
                (SrcAlpha, OneMinusSrcAlpha),
                (SrcAlpha, One),
                (Dst, OneMinusSrcAlpha),
                (One, OneMinusSrc),
            ]
        );
    }
}
