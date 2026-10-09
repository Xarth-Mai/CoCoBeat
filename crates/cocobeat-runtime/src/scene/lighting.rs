//! Original stylized night reflections; no external sky or texture assets

use std::f32::consts::TAU;

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    math::Affine2,
    prelude::*,
    render::render_resource::{
        Extent3d, TextureDataOrder, TextureDimension, TextureFormat, TextureViewDescriptor,
        TextureViewDimension,
    },
};

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
            1.5
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
                        Vec3::new(0.008, 0.012, 0.024).lerp(Vec3::new(0.035, 0.055, 0.11), sky);
                    for (axis, tint) in [
                        (Vec3::new(-0.86, 0.28, -0.35), Vec3::new(0.035, 0.38, 0.62)),
                        (Vec3::new(0.86, 0.28, -0.35), Vec3::new(0.52, 0.075, 0.24)),
                        (Vec3::new(-0.15, 0.70, -0.70), Vec3::new(0.72, 0.70, 0.64)),
                    ] {
                        color += tint * direction.dot(axis.normalize()).max(0.0).powf(spread);
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
        base_color: Color::srgb(0.075, 0.092, 0.13),
        metallic: if texture.is_some() { 1.0 } else { 0.08 },
        perceptual_roughness: if texture.is_some() { 1.0 } else { 0.38 },
        metallic_roughness_texture: texture,
        clearcoat: 0.30,
        clearcoat_perceptual_roughness: 0.24,
        uv_transform: Affine2::from_scale(uv_scale),
        ..default()
    }
}

fn wet_roughness() -> Image {
    let mut bytes = Vec::with_capacity(64 * 64 * 4);
    for y in 0..64 {
        for x in 0..64 {
            let u = (x as f32 + 0.5) / 64.0 * TAU;
            let v = (y as f32 + 0.5) / 64.0 * TAU;
            let patch =
                (0.5 + 0.35 * u.sin() * v.cos() + 0.15 * (3.0 * u + 2.0 * v).cos()).clamp(0.0, 1.0);
            let wet = ((patch - 0.35) / 0.4).clamp(0.0, 1.0);
            let wet = wet * wet * (3.0 - 2.0 * wet);
            // Linear material data: G roughness, B metallic; R/A unused
            bytes.extend([
                0,
                ((0.62 - 0.46 * wet) * 255.0).round() as u8,
                (14.0 + 20.0 * wet).round() as u8,
                255,
            ]);
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: 64,
            height: 64,
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
        assert_eq!(data.len(), 64 * 64 * 4);
        assert!(data.as_chunks::<4>().0.iter().any(|pixel| pixel[1] < 60));
        assert!(data.as_chunks::<4>().0.iter().any(|pixel| pixel[1] > 140));
        assert!(
            data.as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| (14..=34).contains(&pixel[2]) && pixel[3] == 255)
        );
    }
}
