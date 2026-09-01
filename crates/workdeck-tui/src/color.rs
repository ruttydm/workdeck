//! Terminal-theme color math with Hunk-compatible invalid-input fallbacks.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct RgbColor {
    red: u8,
    green: u8,
    blue: u8,
}

fn hex_to_rgb(hex: &str) -> RgbColor {
    let normalized = hex.strip_prefix('#').unwrap_or(hex);
    if normalized.len() != 6 || !normalized.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return RgbColor::default();
    }
    let value = u32::from_str_radix(normalized, 16).unwrap_or_default();
    RgbColor {
        red: ((value >> 16) & 0xff) as u8,
        green: ((value >> 8) & 0xff) as u8,
        blue: (value & 0xff) as u8,
    }
}

fn blend_channel(front: u8, back: u8, ratio: f64) -> u8 {
    let value = f64::from(back) + (f64::from(front) - f64::from(back)) * ratio;
    if value.is_nan() {
        // JavaScript bitwise conversion turns the NaN left by Math.max/min into zero.
        return 0;
    }
    if value == f64::INFINITY {
        return u8::MAX;
    }
    if value == f64::NEG_INFINITY {
        return 0;
    }
    // Math.round chooses the integer toward positive infinity at a half.
    (value + 0.5).floor().clamp(0.0, 255.0) as u8
}

/// Blend foreground toward background at `ratio`, returning lowercase `#rrggbb`.
#[must_use]
pub fn blend_hex(foreground: &str, background: &str, ratio: f64) -> String {
    let foreground = hex_to_rgb(foreground);
    let background = hex_to_rgb(background);
    format!(
        "#{:02x}{:02x}{:02x}",
        blend_channel(foreground.red, background.red, ratio),
        blend_channel(foreground.green, background.green, ratio),
        blend_channel(foreground.blue, background.blue, ratio)
    )
}

fn linearized_channel(channel: u8) -> f64 {
    let value = f64::from(channel) / 255.0;
    if value <= 0.039_28 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// WCAG relative luminance for a six-digit hexadecimal color.
#[must_use]
pub fn relative_luminance(hex: &str) -> f64 {
    let color = hex_to_rgb(hex);
    0.2126 * linearized_channel(color.red)
        + 0.7152 * linearized_channel(color.green)
        + 0.0722 * linearized_channel(color.blue)
}

/// WCAG contrast ratio between two six-digit hexadecimal colors.
#[must_use]
pub fn contrast_ratio(foreground: &str, background: &str) -> f64 {
    let foreground = relative_luminance(foreground);
    let background = relative_luminance(background);
    let lighter = foreground.max(background);
    let darker = foreground.min(background);
    (lighter + 0.05) / (darker + 0.05)
}

/// Manhattan RGB channel distance used for theme separation heuristics.
#[must_use]
pub fn hex_color_distance(left: &str, right: &str) -> u16 {
    let left = hex_to_rgb(left);
    let right = hex_to_rgb(right);
    u16::from(left.red.abs_diff(right.red))
        + u16::from(left.green.abs_diff(right.green))
        + u16::from(left.blue.abs_diff(right.blue))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_uses_javascript_rounding_and_black_invalid_fallbacks() {
        assert_eq!(blend_hex("#ffffff", "#000000", 0.5), "#808080");
        assert_eq!(blend_hex("ff0000", "0000ff", 0.5), "#800080");
        assert_eq!(blend_hex("#123456", "#abcdef", 0.0), "#abcdef");
        assert_eq!(blend_hex("#123456", "#abcdef", 1.0), "#123456");
        assert_eq!(blend_hex("invalid", "#ffffff", 1.0), "#000000");
        assert_eq!(blend_hex("#ffffff", "invalid", 0.0), "#000000");
        assert_eq!(blend_hex("#ffffff", "#ffffff", f64::NAN), "#000000");
    }

    #[test]
    fn luminance_contrast_and_distance_match_wcag_math() {
        assert_eq!(relative_luminance("#000000"), 0.0);
        assert_eq!(relative_luminance("#ffffff"), 1.0);
        assert!((contrast_ratio("#000000", "#ffffff") - 21.0).abs() < f64::EPSILON);
        assert_eq!(hex_color_distance("#000000", "#ffffff"), 765);
        assert_eq!(hex_color_distance("not-a-color", "#000000"), 0);
    }
}
