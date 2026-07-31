#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContentRect {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

pub fn normalize_pointer(x: f32, y: f32, rect: ContentRect) -> (f32, f32) {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return (0.0, 0.0);
    }

    let normalized_x = ((x - rect.left) / rect.width).clamp(0.0, 1.0);
    let normalized_y = ((y - rect.top) / rect.height).clamp(0.0, 1.0);
    (normalized_x, normalized_y)
}

pub fn map_normalized_pointer(
    normalized_x: f32,
    normalized_y: f32,
    rect: ContentRect,
) -> (i32, i32) {
    let x = rect.left + normalized_x.clamp(0.0, 1.0) * rect.width;
    let y = rect.top + normalized_y.clamp(0.0, 1.0) * rect.height;
    (x.round() as i32, y.round() as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_coordinates_are_clamped_to_content() {
        let rect = ContentRect {
            left: 100.0,
            top: 50.0,
            width: 800.0,
            height: 600.0,
        };
        assert_eq!(normalize_pointer(100.0, 50.0, rect), (0.0, 0.0));
        assert_eq!(normalize_pointer(900.0, 650.0, rect), (1.0, 1.0));
        assert_eq!(normalize_pointer(20.0, 900.0, rect), (0.0, 1.0));
    }

    #[test]
    fn coordinates_round_trip_between_different_dpi_surfaces() {
        let receiver = ContentRect {
            left: 0.0,
            top: 0.0,
            width: 1440.0,
            height: 810.0,
        };
        let source = ContentRect {
            left: 1920.0,
            top: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let normalized = normalize_pointer(720.0, 405.0, receiver);
        assert_eq!(
            map_normalized_pointer(normalized.0, normalized.1, source),
            (2880, 540)
        );
    }
}
