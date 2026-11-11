//! Sequential GIF composition. Memory depends on canvas size, not frame count.
use super::*;
use gif::{ColorOutput, DecodeOptions, DisposalMethod};
use std::{cell::RefCell, collections::VecDeque, time::SystemTime};

thread_local! { static SESSION: RefCell<Option<GifSession>> = const { RefCell::new(None) }; }

pub(super) fn clear() {
    SESSION.with(|slot| {
        slot.borrow_mut().take();
    });
}

struct GifSession {
    path: PathBuf,
    stamp: Option<SystemTime>,
    size_on_disk: u64,
    decoder: gif::Decoder<fs::File>,
    canvas: PixelBuffer,
    restore: Option<PixelBuffer>,
    previous_disposal: DisposalMethod,
    previous_rect: (u32, u32, u32, u32),
    next: usize,
    count: usize,
    width: u32,
    height: u32,
    _scratch: crate::resources::Reservation,
    cached: VecDeque<(usize, Frame)>,
    cache_bytes: usize,
}

impl GifSession {
    fn open(path: &Path, cancel: &impl Fn() -> bool) -> ImageResult<Self> {
        let metadata = fs::metadata(path)?;
        let mut info_options = DecodeOptions::new();
        info_options.skip_frame_decoding(true);
        let mut info = info_options
            .read_info(fs::File::open(path)?)
            .map_err(|e| ImageError(e.to_string()))?;
        let mut count = 0;
        while count < MAX_FRAMES as usize
            && info
                .next_frame_info()
                .map_err(|e| ImageError(e.to_string()))?
                .is_some()
        {
            if cancel() {
                return Err(ImageError("Carga cancelada.".into()));
            }
            count += 1;
        }
        if count == 0 {
            return Err(ImageError("El GIF no contiene fotogramas.".into()));
        }
        let mut options = DecodeOptions::new();
        options.set_color_output(ColorOutput::RGBA);
        let decoder = options
            .read_info(fs::File::open(path)?)
            .map_err(|e| ImageError(e.to_string()))?;
        let (width, height) = (u32::from(decoder.width()), u32::from(decoder.height()));
        validate_dimensions(width, height)?;
        let canvas = PixelBuffer::zeroed(width as usize * height as usize * 4)?;
        let scratch = crate::resources::cpu_budget()
            .reserve((width as usize * height as usize * 4).min(50_000_000))?;
        Ok(Self {
            path: path.to_path_buf(),
            stamp: metadata.modified().ok(),
            size_on_disk: metadata.len(),
            decoder,
            canvas,
            restore: None,
            previous_disposal: DisposalMethod::Keep,
            previous_rect: (0, 0, 0, 0),
            next: 0,
            count,
            width,
            height,
            _scratch: scratch,
            cached: VecDeque::new(),
            cache_bytes: 0,
        })
    }

    fn read(&mut self, page: usize, cancel: &impl Fn() -> bool) -> ImageResult<Frame> {
        let mut delay = 100;
        while self.next <= page {
            if cancel() {
                return Err(ImageError("Carga cancelada.".into()));
            }
            match self.previous_disposal {
                DisposalMethod::Background => clear_rect_cancellable(
                    self.canvas.try_make_mut()?,
                    self.width,
                    self.previous_rect,
                    cancel,
                )?,
                DisposalMethod::Previous => {
                    if let Some(previous) = self.restore.take() {
                        self.canvas = previous;
                    }
                }
                _ => {}
            }
            let frame = self
                .decoder
                .read_next_frame()
                .map_err(|e| ImageError(e.to_string()))?
                .ok_or_else(|| ImageError("Fotograma GIF no disponible.".into()))?;
            let (l, t, w, h) = (
                u32::from(frame.left),
                u32::from(frame.top),
                u32::from(frame.width),
                u32::from(frame.height),
            );
            if l + w > self.width || t + h > self.height {
                return Err(ImageError("Fotograma GIF fuera del lienzo lógico.".into()));
            }
            self.restore = if frame.dispose == DisposalMethod::Previous {
                Some(self.canvas.clone())
            } else {
                None
            };
            // Scratch is reserved for the lifetime of the decoder's RGBA buffer.
            let canvas = self.canvas.try_make_mut()?;
            for y in 0..h {
                if y % 32 == 0 && cancel() {
                    return Err(ImageError("Carga cancelada.".into()));
                }
                for x in 0..w {
                    let s = ((y * w + x) * 4) as usize;
                    if frame.buffer[s + 3] != 0 {
                        let d = (((t + y) * self.width + l + x) * 4) as usize;
                        canvas[d..d + 4].copy_from_slice(&[
                            frame.buffer[s + 2],
                            frame.buffer[s + 1],
                            frame.buffer[s],
                            frame.buffer[s + 3],
                        ]);
                    }
                }
            }
            self.previous_disposal = frame.dispose;
            self.previous_rect = (l, t, w, h);
            delay = u32::from(frame.delay).saturating_mul(10).max(20);
            self.next += 1;
        }
        let frame = Frame {
            width: self.width,
            height: self.height,
            pixels: self.canvas.clone(),
            delay_ms: delay,
        };
        let bytes = frame.byte_len();
        if bytes <= 32 * crate::resources::MIB {
            while self.cache_bytes + bytes > 32 * crate::resources::MIB {
                if let Some((_, evicted)) = self.cached.pop_front() {
                    self.cache_bytes -= evicted.byte_len();
                } else {
                    break;
                }
            }
            self.cached.push_back((page, frame.clone()));
            self.cache_bytes += bytes;
        }
        Ok(frame)
    }
}

pub(super) fn decode_gif(
    path: &Path,
    cancel: &impl Fn() -> bool,
    request: &DecodeRequest,
) -> ImageResult<ImageDocument> {
    SESSION.with(|slot| {
        let mut slot = slot.borrow_mut();
        let metadata = fs::metadata(path)?;
        if let Some(session) = slot.as_ref()
            && session.path == path
            && session.stamp == metadata.modified().ok()
            && session.size_on_disk == metadata.len()
            && let Some((page, frame)) = session
                .cached
                .iter()
                .find(|(page, _)| *page == request.page.min(session.count - 1))
        {
            let original = (session.width, session.height);
            let frame = render_raster_view(frame, original, request, cancel)?;
            return Ok(raster_document(
                path,
                frame,
                original,
                session.count,
                *page,
                "GIF",
                request,
            ));
        }
        let reset = slot.as_ref().is_none_or(|session| {
            session.path != path
                || session.stamp != metadata.modified().ok()
                || session.size_on_disk != metadata.len()
                || request.page < session.next
        });
        if reset {
            slot.take();
            *slot = Some(GifSession::open(path, cancel)?);
        }
        let session = slot.as_mut().expect("GIF session");
        let page = request.page.min(session.count - 1);
        let count = session.count;
        let original = (session.width, session.height);
        let frame = match session.read(page, cancel) {
            Ok(frame) => frame,
            Err(error) => {
                *slot = None;
                return Err(error);
            }
        };
        let frame = render_raster_view(&frame, original, request, cancel)?;
        Ok(raster_document(
            path, frame, original, count, page, "GIF", request,
        ))
    })
}
