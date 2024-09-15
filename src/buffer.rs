use std::{
    collections::VecDeque,
    fmt::Debug,
    sync::{atomic::AtomicUsize, Arc},
};

use crate::parser::INIT_BUF_SIZE;
// Set a reasonable value to avoid causing frequent memory allocations
const MAX_REUSE_BUF_SIZE: usize = 1024 * 1024;
const MAX_POOLED_BUF: usize = 16;

pub(crate) struct Buffers {
    shared: VecDeque<Arc<Vec<u8>>>,
    pool: VecDeque<Vec<u8>>,
    acquired: AtomicUsize,
}

impl Buffers {
    pub fn new() -> Self {
        Self::default()
    }

    #[tracing::instrument(skip_all)]
    pub fn release(&mut self, mut buf: Vec<u8>) {
        if self.pooled() > MAX_POOLED_BUF {
            // buf dropped
        } else {
            // buf pooled
            Self::clean(&mut buf);
            self.pool.push_back(buf);
        }
        self.checked_sub_acquired();
        tracing::debug!(?self, "buffers status");
    }

    #[tracing::instrument(skip_all)]
    pub fn release_to_share(&mut self, buf: Vec<u8>) -> Arc<Vec<u8>> {
        let arc = Arc::new(buf);
        self.shared.push_back(arc.clone());
        self.checked_sub_acquired();
        tracing::debug!(?self, "buffers status");
        arc
    }

    #[tracing::instrument(skip_all)]
    pub fn acquire(&mut self) -> Vec<u8> {
        // try to recyle shared buffers first
        let buf = if let Some(buf) = self.recycle() {
            tracing::debug!(?self, "acquired: recycled");
            buf
        } else if let Some(buf) = self.pool.pop_back() {
            tracing::debug!(?self, "acquired: pooled");
            buf
        } else {
            tracing::debug!(?self, "acquired: created");
            new_buf()
        };

        let prev = self
            .acquired
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if prev == usize::MAX {
            panic!("too many acquired buffers");
        }

        tracing::debug!(?self, "buffers status");

        buf
    }

    fn recycle(&mut self) -> Option<Vec<u8>> {
        let arc_idx = self
            .shared
            .iter_mut()
            .enumerate()
            .find_map(|(i, x)| Arc::get_mut(x).and(Some(i)));
        arc_idx.and_then(|i| {
            self.shared.remove(i).and_then(|arc| {
                match Arc::try_unwrap(arc) {
                    Ok(mut buf) => {
                        // recycled
                        Self::clean(&mut buf);
                        Some(buf)
                    }
                    Err(arc) => {
                        // still being used, put it back
                        self.shared.push_back(arc);
                        None
                    }
                }
            })
        })
    }

    fn shared(&self) -> usize {
        self.shared.len()
    }

    fn pooled(&self) -> usize {
        self.pool.len()
    }

    fn acquired(&self) -> usize {
        self.acquired.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn clean(buf: &mut Vec<u8>) {
        buf.clear();
        if buf.capacity() > MAX_REUSE_BUF_SIZE {
            buf.shrink_to(MAX_REUSE_BUF_SIZE);
        }
    }

    fn checked_sub_acquired(&mut self) {
        let prev = self
            .acquired
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        if prev == 0 {
            tracing::error!("released wrong buf");
            panic!("released wrong buf");
        }
    }
}

impl Default for Buffers {
    fn default() -> Self {
        let mut pool = VecDeque::new();
        pool.push_back(new_buf());
        Self {
            shared: VecDeque::new(),
            pool,
            acquired: AtomicUsize::new(0),
        }
    }
}

fn new_buf() -> Vec<u8> {
    Vec::with_capacity(INIT_BUF_SIZE)
}

impl Debug for Buffers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Buffer")
            .field("acquired", &self.acquired())
            .field("shared", &self.shared.len())
            .field("pool", &self.pool.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::Buffers;

    #[test]
    fn buffers() {
        let mut bb = Buffers::new();

        let buf1 = bb.acquire();
        assert_eq!(bb.acquired(), 1);
        assert_eq!(bb.pooled(), 0);

        let buf = bb.acquire();
        assert_eq!(bb.acquired(), 2);

        let shared = bb.release_to_share(buf);
        assert_eq!(bb.acquired(), 1);
        assert_eq!(bb.shared(), 1);

        let buf = bb.acquire();
        assert_eq!(bb.acquired(), 2);
        assert_eq!(bb.shared(), 1);

        bb.release(buf);
        assert_eq!(bb.acquired(), 1);
        assert_eq!(bb.shared(), 1);
        assert_eq!(bb.pooled(), 1);

        drop(shared);

        bb.acquire();
        assert_eq!(bb.acquired(), 2);
        assert_eq!(bb.shared(), 0);
        assert_eq!(bb.pooled(), 1);

        bb.release_to_share(buf1);
        assert_eq!(bb.acquired(), 1);
        assert_eq!(bb.shared(), 1);
        assert_eq!(bb.pooled(), 1);
    }
}
