//! Lossless geometry history in original-image coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditOperation {
    RotateRight,
    RotateLeft,
    FlipHorizontal,
    FlipVertical,
    Crop(u32, u32, u32, u32),
}

#[derive(Clone, Debug, Default)]
pub struct EditHistory {
    pub operations: Vec<EditOperation>,
    redo: Vec<EditOperation>,
}

impl EditHistory {
    pub fn from_operations(operations: Vec<EditOperation>) -> Self {
        Self {
            operations,
            redo: Vec::new(),
        }
    }
    pub fn push(&mut self, operation: EditOperation) {
        self.operations.push(operation);
        self.redo.clear();
    }
    pub fn undo(&mut self) -> bool {
        if let Some(operation) = self.operations.pop() {
            self.redo.push(operation);
            true
        } else {
            false
        }
    }
    pub fn redo(&mut self) -> bool {
        if let Some(operation) = self.redo.pop() {
            self.operations.push(operation);
            true
        } else {
            false
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewTransform {
    pub width: u32,
    pub height: u32,
    xx: i64,
    xy: i64,
    yx: i64,
    yy: i64,
    ox: i64,
    oy: i64,
}

impl ViewTransform {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            xx: 1,
            xy: 0,
            yx: 0,
            yy: 1,
            ox: 0,
            oy: 0,
        }
    }

    pub fn from_operations(size: (u32, u32), operations: &[EditOperation]) -> Self {
        let mut view = Self::new(size.0, size.1);
        for &operation in operations {
            view.apply(operation);
        }
        view
    }

    fn compose(&mut self, matrix: [i64; 6], width: u32, height: u32) {
        let [a, b, c, d, x, y] = matrix;
        *self = Self {
            xx: self.xx * a + self.xy * c,
            xy: self.xx * b + self.xy * d,
            yx: self.yx * a + self.yy * c,
            yy: self.yx * b + self.yy * d,
            ox: self.ox + self.xx * x + self.xy * y,
            oy: self.oy + self.yx * x + self.yy * y,
            width,
            height,
        };
    }

    pub fn apply(&mut self, operation: EditOperation) {
        let (w, h) = (self.width, self.height);
        match operation {
            EditOperation::RotateRight => self.compose([0, 1, -1, 0, 0, i64::from(h) - 1], h, w),
            EditOperation::RotateLeft => self.compose([0, -1, 1, 0, i64::from(w) - 1, 0], h, w),
            EditOperation::FlipHorizontal => self.compose([-1, 0, 0, 1, i64::from(w) - 1, 0], w, h),
            EditOperation::FlipVertical => self.compose([1, 0, 0, -1, 0, i64::from(h) - 1], w, h),
            EditOperation::Crop(l, t, r, b) => {
                let (l, t, r, b) = (l.min(w), t.min(h), r.min(w), b.min(h));
                if r > l && b > t {
                    self.compose([1, 0, 0, 1, i64::from(l), i64::from(t)], r - l, b - t);
                }
            }
        }
    }

    pub fn source_pixel(&self, x: u32, y: u32) -> (u32, u32) {
        let (x, y) = (i64::from(x), i64::from(y));
        (
            (self.xx * x + self.xy * y + self.ox) as u32,
            (self.yx * x + self.yy * y + self.oy) as u32,
        )
    }

    pub fn inverse_matrix(&self) -> [f32; 6] {
        let ox = self.ox as f32 + 0.5 - (self.xx + self.xy) as f32 * 0.5;
        let oy = self.oy as f32 + 0.5 - (self.yx + self.yy) as f32 * 0.5;
        [
            self.xx as f32,
            self.xy as f32,
            self.yx as f32,
            self.yy as f32,
            -(self.xx as f32 * ox + self.yx as f32 * oy),
            -(self.xy as f32 * ox + self.yy as f32 * oy),
        ]
    }

    pub fn orientation(&self) -> u32 {
        match (self.xx, self.xy, self.yx, self.yy) {
            (-1, 0, 0, 1) => 2,
            (-1, 0, 0, -1) => 3,
            (1, 0, 0, -1) => 4,
            (0, 1, 1, 0) => 5,
            (0, 1, -1, 0) => 6,
            (0, -1, -1, 0) => 7,
            (0, -1, 1, 0) => 8,
            _ => 1,
        }
    }

    pub fn source_bounds(&self) -> (u32, u32, u32, u32) {
        let points = [
            self.source_pixel(0, 0),
            self.source_pixel(self.width - 1, 0),
            self.source_pixel(0, self.height - 1),
            self.source_pixel(self.width - 1, self.height - 1),
        ];
        (
            points.iter().map(|p| p.0).min().unwrap(),
            points.iter().map(|p| p.1).min().unwrap(),
            points.iter().map(|p| p.0).max().unwrap() + 1,
            points.iter().map(|p| p.1).max().unwrap() + 1,
        )
    }
}

/// Never enlarge a source and always respect a pixel allocation ceiling.
pub fn fitted_size(size: (u32, u32), target: (u32, u32), max_bytes: usize) -> (u32, u32) {
    let (w, h) = (size.0.max(1), size.1.max(1));
    let scale = (f64::from(target.0.max(1)) / f64::from(w))
        .min(f64::from(target.1.max(1)) / f64::from(h))
        .min((max_bytes as f64 / (f64::from(w) * f64::from(h) * 4.0)).sqrt())
        .min(1.0);
    (
        (f64::from(w) * scale).floor().max(1.0) as u32,
        (f64::from(h) * scale).floor().max(1.0) as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_after_rotation_maps_to_original_pixels() {
        let view = ViewTransform::from_operations(
            (3, 2),
            &[EditOperation::RotateRight, EditOperation::Crop(0, 1, 2, 3)],
        );
        assert_eq!((view.width, view.height), (2, 2));
        assert_eq!(view.source_pixel(0, 0), (1, 1));
        assert_eq!(view.source_pixel(1, 1), (2, 0));
    }

    #[test]
    fn four_rotations_are_identity() {
        let view = ViewTransform::from_operations((7, 3), &[EditOperation::RotateRight; 4]);
        assert_eq!(view, ViewTransform::new(7, 3));
    }

    #[test]
    fn preview_size_is_bounded_and_preserves_aspect() {
        assert_eq!(
            fitted_size((10000, 6000), (1920, 1080), 32 * 1024 * 1024),
            (1800, 1080)
        );
        let (w, h) = fitted_size((100000, 100000), (100000, 100000), 16 * 1024 * 1024);
        assert!(u64::from(w) * u64::from(h) * 4 <= 16 * 1024 * 1024);
    }
}
