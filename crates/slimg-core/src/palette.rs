use std::collections::HashMap;

use crate::codec::ImageData;

const UNIQUE_COLOR_LIMIT: usize = 1_000_000;

/// Recommendation for palette-based PNG compression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteRecommendation {
    /// Palette compression is likely useful.
    On,
    /// Palette compression may be useful and should be checked with a candidate encode.
    Review,
    /// Palette compression is unlikely to be useful.
    Off,
}

/// Color statistics used to assess palette-based PNG compression suitability.
#[derive(Debug, Clone, PartialEq)]
pub struct PaletteStats {
    /// Exact unique RGBA color count while below the tracking limit.
    pub unique_color_count: u32,
    /// Whether unique color tracking exceeded the internal memory guard.
    pub unique_color_count_exceeded: bool,
    /// Fraction of pixels covered by the 256 most frequent tracked colors.
    pub top_256_color_coverage: f64,
    /// Whether any source pixel has alpha below 255.
    pub has_alpha: bool,
    /// Conservative recommendation based on color statistics.
    pub recommendation: PaletteRecommendation,
}

/// Analyze whether an RGBA image is suitable for palette-based PNG compression.
pub fn analyze_palette_suitability(image: &ImageData) -> PaletteStats {
    let total_pixels = (image.data.len() / 4) as u64;
    if total_pixels == 0 {
        return PaletteStats {
            unique_color_count: 0,
            unique_color_count_exceeded: false,
            top_256_color_coverage: 0.0,
            has_alpha: false,
            recommendation: PaletteRecommendation::Off,
        };
    }

    let mut counts: HashMap<u32, u32> = HashMap::new();
    let mut has_alpha = false;

    for pixel in image.data.chunks_exact(4) {
        has_alpha |= pixel[3] < 255;
        let color = pack_rgba(pixel);
        if !counts.contains_key(&color) && counts.len() >= UNIQUE_COLOR_LIMIT {
            return PaletteStats {
                unique_color_count: counts.len() as u32,
                unique_color_count_exceeded: true,
                top_256_color_coverage: 0.0,
                has_alpha,
                recommendation: PaletteRecommendation::Off,
            };
        }
        *counts.entry(color).or_insert(0) += 1;
    }

    let unique_color_count = counts.len() as u32;
    let top_256_color_coverage = top_color_coverage(&counts, total_pixels);
    let recommendation = if unique_color_count <= 256 {
        PaletteRecommendation::On
    } else if top_256_color_coverage >= 0.98 {
        PaletteRecommendation::On
    } else if top_256_color_coverage >= 0.90 {
        PaletteRecommendation::Review
    } else {
        PaletteRecommendation::Off
    };

    PaletteStats {
        unique_color_count,
        unique_color_count_exceeded: false,
        top_256_color_coverage,
        has_alpha,
        recommendation,
    }
}

fn pack_rgba(pixel: &[u8]) -> u32 {
    u32::from_be_bytes([pixel[0], pixel[1], pixel[2], pixel[3]])
}

fn top_color_coverage(counts: &HashMap<u32, u32>, total_pixels: u64) -> f64 {
    let mut values = counts.values().copied().collect::<Vec<_>>();
    values.sort_unstable_by(|a, b| b.cmp(a));
    let top_count = values.into_iter().take(256).map(u64::from).sum::<u64>();
    top_count as f64 / total_pixels as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommends_palette_for_low_color_image() {
        let image = ImageData::new(
            2,
            2,
            vec![
                255, 0, 0, 255, 255, 0, 0, 255, 0, 0, 255, 255, 0, 0, 255, 255,
            ],
        );

        let stats = analyze_palette_suitability(&image);

        assert_eq!(stats.unique_color_count, 2);
        assert_eq!(stats.recommendation, PaletteRecommendation::On);
    }

    #[test]
    fn reports_alpha() {
        let image = ImageData::new(1, 1, vec![0, 0, 0, 0]);

        let stats = analyze_palette_suitability(&image);

        assert!(stats.has_alpha);
    }
}
