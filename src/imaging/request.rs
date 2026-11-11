use super::view::{EditOperation, ViewTransform, fitted_size};

#[derive(Clone, Debug)]
pub struct DecodeRequest {
    pub page: usize,
    pub target: (u32, u32),
    pub region: Option<(u32, u32, u32, u32)>,
    pub operations: Vec<EditOperation>,
    pub max_bytes: usize,
    pub allow_downsample: bool,
}

impl DecodeRequest {
    pub fn original(page: usize) -> Self {
        Self {
            page,
            target: (100_000, 100_000),
            region: None,
            operations: Vec::new(),
            max_bytes: crate::resources::CPU_LIMIT,
            allow_downsample: false,
        }
    }
    pub fn preview(target: (u32, u32)) -> Self {
        Self {
            target,
            max_bytes: crate::resources::PREVIEW_LIMIT,
            allow_downsample: true,
            ..Self::original(0)
        }
    }
    pub fn layout(&self, original: (u32, u32)) -> ViewLayout {
        let view = ViewTransform::from_operations(original, &self.operations);
        let (l, t, r, b) = self.region.unwrap_or((0, 0, view.width, view.height));
        let l = l.min(view.width - 1);
        let t = t.min(view.height - 1);
        let r = r.clamp(l + 1, view.width);
        let b = b.clamp(t + 1, view.height);
        let size = if self.allow_downsample {
            fitted_size((r - l, b - t), self.target, self.max_bytes)
        } else {
            (r - l, b - t)
        };
        ViewLayout {
            region: (l, t, r, b),
            size,
        }
    }
}

pub struct ViewLayout {
    pub region: (u32, u32, u32, u32),
    pub size: (u32, u32),
}
