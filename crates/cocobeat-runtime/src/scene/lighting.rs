//! Original stylized night reflections; no external sky or texture assets

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    math::Affine2,
    post_process::bloom::{Bloom, BloomCompositeMode, BloomPrefilter},
    prelude::*,
    render::render_resource::{
        Extent3d, TextureDataOrder, TextureDimension, TextureFormat, TextureViewDescriptor,
        TextureViewDimension,
    },
};

use crate::presentation::WorldTheme;

pub(super) fn atmosphere(world: WorldTheme) -> Color {
    match world {
        WorldTheme::Neon => Color::srgb(0.10, 0.085, 0.17),
        WorldTheme::Forest => Color::srgb(0.055, 0.14, 0.115),
        WorldTheme::Candy => Color::srgb(0.20, 0.095, 0.17),
        WorldTheme::StarSea => Color::srgb(0.065, 0.06, 0.17),
    }
}

/// Thresholded glow leaves skin and the cue lane sharp while emitting surfaces scatter
pub(super) fn bloom(reduced_flashes: bool) -> Bloom {
    Bloom {
        intensity: if reduced_flashes { 0.08 } else { 0.22 },
        low_frequency_boost: 0.25,
        low_frequency_boost_curvature: 0.65,
        prefilter: BloomPrefilter {
            threshold: 1.0,
            threshold_softness: 0.45,
        },
        composite_mode: BloomCompositeMode::Additive,
        scale: Vec2::new(1.25, 1.0),
        ..Bloom::NATURAL
    }
}

#[derive(Resource)]
pub(crate) struct LightingAssets {
    diffuse_map: Handle<Image>,
    specular_map: Handle<Image>,
    wet_map: Handle<Image>,
}

impl LightingAssets {
    pub(super) fn new(images: &mut Assets<Image>) -> Self {
        Self {
            diffuse_map: images.add(night_cube(16, 1, true)),
            specular_map: images.add(night_cube(32, 6, false)),
            wet_map: images.add(wet_roughness()),
        }
    }

    pub(super) fn environment(&self, intensity: f32) -> EnvironmentMapLight {
        EnvironmentMapLight {
            diffuse_map: self.diffuse_map.clone(),
            specular_map: self.specular_map.clone(),
            intensity,
            ..default()
        }
    }
}

fn night_cube(size: u32, mips: u32, diffuse: bool) -> Image {
    let mut bytes = Vec::new();
    // MipMajor is mip -> +X,-X,+Y,-Y,+Z,-Z -> row -> pixel
    for mip in 0..mips {
        let side = size >> mip;
        let spread = if diffuse {
            2.8
        } else {
            40.0 / (1.0 + (mip * mip) as f32)
        };
        for face in 0..6 {
            for y in 0..side {
                for x in 0..side {
                    let u = (x as f32 + 0.5) / side as f32 * 2.0 - 1.0;
                    let v = (y as f32 + 0.5) / side as f32 * 2.0 - 1.0;
                    let direction = match face {
                        0 => Vec3::new(1.0, -v, -u),
                        1 => Vec3::new(-1.0, -v, u),
                        2 => Vec3::new(u, 1.0, v),
                        3 => Vec3::new(u, -1.0, -v),
                        4 => Vec3::new(u, -v, 1.0),
                        _ => Vec3::new(-u, -v, -1.0),
                    }
                    .normalize();
                    let sky = (direction.y * 0.5 + 0.5).clamp(0.0, 1.0);
                    let mut color =
                        Vec3::new(0.004, 0.006, 0.014).lerp(Vec3::new(0.018, 0.030, 0.065), sky);
                    for (axis, tint) in [
                        (Vec3::new(-0.80, 0.28, -0.50), Vec3::new(0.018, 0.36, 0.72)),
                        (Vec3::new(0.80, 0.35, -0.30), Vec3::new(0.72, 0.045, 0.28)),
                        (Vec3::new(-0.35, 0.80, 0.48), Vec3::new(0.72, 0.59, 0.43)),
                    ] {
                        color += tint
                            * direction.dot(axis.normalize()).max(0.0).powf(spread)
                            * if diffuse { 0.45 } else { 1.0 };
                    }
                    bytes.extend(
                        color
                            .to_array()
                            .map(|value| (value.clamp(0.0, 1.0) * 255.0).round() as u8),
                    );
                    bytes.push(255);
                }
            }
        }
    }
    // Image::new checks only base-level length, so assign all mips to new_uninit
    let mut image = Image::new_uninit(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(bytes);
    image.data_order = TextureDataOrder::MipMajor;
    image.texture_descriptor.mip_level_count = mips;
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    image.sampler = ImageSampler::linear();
    image
}

pub(super) fn asphalt(assets: Option<&LightingAssets>, uv_scale: Vec2) -> StandardMaterial {
    let texture = assets.map(|assets| assets.wet_map.clone());
    StandardMaterial {
        base_color: Color::srgb(0.065, 0.078, 0.105),
        metallic: 0.0,
        perceptual_roughness: if texture.is_some() { 1.0 } else { 0.38 },
        metallic_roughness_texture: texture,
        clearcoat: 0.45,
        clearcoat_perceptual_roughness: 0.20,
        uv_transform: Affine2::from_scale(uv_scale),
        ..default()
    }
}

fn wet_roughness() -> Image {
    const SIZE: u32 = 128;
    // Periodic ellipses give broken wet patches; low contrast avoids wave-shaped highlights
    let patches = [
        (0.13, 0.22, 0.24, 0.10),
        (0.74, 0.67, 0.19, 0.14),
        (0.44, 0.89, 0.26, 0.09),
        (0.90, 0.14, 0.18, 0.11),
        (0.36, 0.51, 0.15, 0.10),
        (0.08, 0.83, 0.17, 0.06),
    ];
    let mut bytes = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let u = (x as f32 + 0.5) / SIZE as f32;
            let v = (y as f32 + 0.5) / SIZE as f32;
            let wet = patches.iter().fold(0.0_f32, |wet, &(cx, cy, rx, ry)| {
                let dx = ((u - cx + 0.5).rem_euclid(1.0) - 0.5) / rx;
                let dy = ((v - cy + 0.5).rem_euclid(1.0) - 0.5) / ry;
                let edge = (1.0 - dx * dx - dy * dy).clamp(0.0, 1.0);
                wet.max(edge * edge * (3.0 - 2.0 * edge))
            });
            let grain = ((x.wrapping_mul(1973) ^ y.wrapping_mul(9277)).wrapping_mul(26699) & 255)
                as f32
                / 255.0;
            // Asphalt is dielectric; G stores restrained roughness and B stays zero
            bytes.extend([
                0,
                ((0.42 - 0.13 * wet + 0.018 * (grain - 0.5)) * 255.0).round() as u8,
                0,
                255,
            ]);
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        bytes,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_mips_and_wet_channels_keep_native_texture_contract() {
        let mut images = Assets::<Image>::default();
        let assets = LightingAssets::new(&mut images);
        let light = assets.environment(400.0);
        for (handle, size, levels, bytes) in [
            (&light.diffuse_map, 16, 1, 6144),
            (&light.specular_map, 32, 6, 32760),
        ] {
            let image = images.get(handle).unwrap();
            assert_eq!(
                image.texture_descriptor.size,
                Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: 6
                }
            );
            assert_eq!(image.texture_descriptor.mip_level_count, levels);
            assert_eq!(image.data_order, TextureDataOrder::MipMajor);
            assert_eq!(image.texture_descriptor.format, TextureFormat::Rgba8Unorm);
            assert_eq!(
                image.texture_view_descriptor.as_ref().unwrap().dimension,
                Some(TextureViewDimension::Cube)
            );
            assert_eq!(image.data.as_ref().unwrap().len(), bytes);
        }
        let fallback = asphalt(None, Vec2::ONE);
        assert!(fallback.metallic_roughness_texture.is_none());
        let material = asphalt(Some(&assets), Vec2::ONE);
        let image = images
            .get(material.metallic_roughness_texture.as_ref().unwrap())
            .unwrap();
        let data = image.data.as_ref().unwrap();
        assert_eq!(data.len(), 128 * 128 * 4);
        assert!(data.as_chunks::<4>().0.iter().any(|pixel| pixel[1] < 80));
        assert!(data.as_chunks::<4>().0.iter().any(|pixel| pixel[1] > 105));
        assert!(
            data.as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| (70..=110).contains(&pixel[1]) && pixel[2] == 0 && pixel[3] == 255)
        );
    }
}
