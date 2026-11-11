//! Process-wide admission control for pixel buffers. Codec/driver allocations
//! are measured separately; they cannot be constrained by this allocator.
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicUsize, Ordering},
};

pub const MIB: usize = 1024 * 1024;
pub const CPU_LIMIT: usize = 512 * MIB;
pub const GPU_LIMIT: usize = 64 * MIB;
pub const PREVIEW_LIMIT: usize = 32 * MIB;
pub const PREFETCH_LIMIT: usize = 16 * MIB;

#[derive(Debug)]
pub struct ResourceBudget {
    limit: usize,
    used: AtomicUsize,
    peak: AtomicUsize,
}

impl ResourceBudget {
    pub fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            limit,
            used: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
        })
    }

    pub fn reserve(self: &Arc<Self>, bytes: usize) -> Result<Reservation, &'static str> {
        let previous = self
            .used
            .try_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes).filter(|&next| next <= self.limit)
            })
            .map_err(|_| "No hay suficiente memoria disponible para esta operación.")?;
        self.peak.fetch_max(previous + bytes, Ordering::Relaxed);
        Ok(Reservation {
            budget: self.clone(),
            bytes,
        })
    }

    pub fn used(&self) -> usize {
        self.used.load(Ordering::Acquire)
    }
    pub fn peak(&self) -> usize {
        self.peak.load(Ordering::Relaxed)
    }
}

#[derive(Debug)]
pub struct Reservation {
    budget: Arc<ResourceBudget>,
    bytes: usize,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.budget.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

pub fn cpu_budget() -> &'static Arc<ResourceBudget> {
    static BUDGET: OnceLock<Arc<ResourceBudget>> = OnceLock::new();
    BUDGET.get_or_init(|| ResourceBudget::new(CPU_LIMIT))
}

pub fn gpu_budget() -> &'static Arc<ResourceBudget> {
    static BUDGET: OnceLock<Arc<ResourceBudget>> = OnceLock::new();
    BUDGET.get_or_init(|| ResourceBudget::new(GPU_LIMIT))
}

/// Immutable shared pixels: cloning a frame reserves no additional memory.
#[derive(Clone, Debug)]
pub struct PixelBuffer(Arc<PixelStorage>);

#[derive(Debug)]
struct PixelStorage {
    data: Vec<u8>,
    _memory: Reservation,
}

impl PixelBuffer {
    pub fn zeroed(bytes: usize) -> Result<Self, &'static str> {
        let memory = cpu_budget().reserve(bytes)?;
        let mut data = Vec::new();
        data.try_reserve_exact(bytes)
            .map_err(|_| "No se pudo reservar memoria para la imagen.")?;
        data.resize(bytes, 0);
        Ok(Self(Arc::new(PixelStorage {
            data,
            _memory: memory,
        })))
    }

    #[cfg(test)]
    pub fn from_vec(data: Vec<u8>) -> Result<Self, &'static str> {
        let memory = cpu_budget().reserve(data.capacity())?;
        Ok(Self(Arc::new(PixelStorage {
            data,
            _memory: memory,
        })))
    }

    pub fn with_reservation(data: Vec<u8>, memory: Reservation) -> Result<Self, &'static str> {
        if data.capacity() > memory.bytes {
            return Err("La imagen supera la memoria reservada.");
        }
        Ok(Self(Arc::new(PixelStorage {
            data,
            _memory: memory,
        })))
    }

    pub fn try_make_mut(&mut self) -> Result<&mut [u8], &'static str> {
        if Arc::get_mut(&mut self.0).is_none() {
            let mut copy = Self::zeroed(self.len())?;
            copy.try_make_mut()?.copy_from_slice(self);
            *self = copy;
        }
        Ok(Arc::get_mut(&mut self.0)
            .expect("unique pixel storage")
            .data
            .as_mut_slice())
    }

    #[cfg(test)]
    pub fn shares_storage(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::ops::Deref for PixelBuffer {
    type Target = Vec<u8>;
    fn deref(&self) -> &Self::Target {
        &self.0.data
    }
}

impl PartialEq for PixelBuffer {
    fn eq(&self, other: &Self) -> bool {
        self.0.data == other.0.data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reservation_is_atomic_and_released() {
        let budget = ResourceBudget::new(12);
        let reservation = budget.reserve(8).unwrap();
        assert!(budget.reserve(5).is_err());
        assert!(budget.reserve(usize::MAX).is_err());
        assert_eq!(budget.used(), 8);
        drop(reservation);
        assert_eq!(budget.used(), 0);
        assert_eq!(budget.peak(), 8);
    }

    #[test]
    fn shared_pixels_copy_only_when_written() {
        let mut first = PixelBuffer::from_vec(vec![1, 2, 3]).unwrap();
        let second = first.clone();
        assert!(first.shares_storage(&second));
        first.try_make_mut().unwrap()[0] = 9;
        assert!(!first.shares_storage(&second));
        assert_eq!(&second[..], &[1, 2, 3]);
    }

    #[test]
    fn concurrent_workers_cannot_overcommit_the_budget() {
        let budget = ResourceBudget::new(128);
        let admitted = Arc::new(std::sync::Barrier::new(33));
        let release = Arc::new(std::sync::Barrier::new(33));
        let workers: Vec<_> = (0..32)
            .map(|_| {
                let (budget, admitted, release) =
                    (budget.clone(), admitted.clone(), release.clone());
                std::thread::spawn(move || {
                    let reservation = budget.reserve(16).ok();
                    admitted.wait();
                    release.wait();
                    drop(reservation);
                })
            })
            .collect();
        admitted.wait();
        assert_eq!(budget.used(), 128);
        assert_eq!(budget.peak(), 128);
        release.wait();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(budget.used(), 0);
    }
}
