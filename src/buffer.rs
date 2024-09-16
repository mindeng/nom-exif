use std::{
    collections::VecDeque,
    fmt::Debug,
    sync::{atomic::AtomicUsize, Arc},
};

use crate::parser::INIT_BUF_SIZE;

// Set a reasonable value to avoid causing frequent memory allocations
const MAX_REUSE_BUF_SIZE: usize = 1024 * 1024;
const INIT_POOLED_BUF: usize = 2;
const MAX_POOLED_BUF: usize = 8;

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
        if self.pooled() >= MAX_POOLED_BUF {
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
        let buf = if let Some(buf) = self.pool.pop_front() {
            tracing::debug!(?self, "acquired: pooled");
            buf
        } else if let Some(buf) = self.recycle() {
            tracing::debug!(?self, "acquired: recycled");
            buf
        } else {
            tracing::debug!(?self, "acquired: new");
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
        let mut remain = VecDeque::new();
        let buf = loop {
            let Some(arc) = self.shared.pop_front() else {
                break None;
            };
            match Arc::try_unwrap(arc) {
                Ok(mut buf) => {
                    // recycled
                    Self::clean(&mut buf);
                    break Some(buf);
                }
                Err(arc) => {
                    // still being used, put it back
                    remain.push_back(arc);
                }
            }
        };
        self.shared.append(&mut remain);
        buf
    }

    #[allow(unused)]
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
            debug_assert!(false, "released wrong buf");
        }
    }
}

impl Default for Buffers {
    fn default() -> Self {
        let mut pool = VecDeque::new();
        for _ in 0..INIT_POOLED_BUF {
            pool.push_back(new_buf());
        }
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
        f.debug_struct("Buffers")
            .field("acquired", &self.acquired())
            .field("shared", &self.shared.len())
            .field("pool", &self.pool.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::min;

    use crate::buffer::{INIT_POOLED_BUF, MAX_POOLED_BUF};

    use super::Buffers;

    #[test]
    fn buffers_prior_to_take_pooled() {
        let mut bb = Buffers::new();
        assert_eq!(bb.acquired(), 0);
        assert_eq!(bb.pooled(), INIT_POOLED_BUF);
        assert_eq!(bb.shared(), 0);

        const NUM: usize = MAX_POOLED_BUF + 1;
        let mut bufs = Vec::with_capacity(2 * NUM);

        for i in 1..=2 * NUM {
            let buf = bb.acquire();
            assert_eq!(bb.acquired(), i);
            assert_eq!(bb.shared(), 0);
            assert_eq!(bb.pooled(), INIT_POOLED_BUF.saturating_sub(i));
            bufs.push(buf);
        }
        assert_eq!(bb.acquired(), 2 * NUM);
        assert_eq!(bb.shared(), 0);
        assert_eq!(bb.pooled(), 0);

        let mut shared = Vec::with_capacity(NUM);
        for i in 1..=NUM {
            let arc = bb.release_to_share(bufs.pop().unwrap());
            assert_eq!(bb.acquired(), 2 * NUM - i);
            assert_eq!(bb.shared(), i);
            assert_eq!(bb.pooled(), 0);
            shared.push(arc);
        }
        assert_eq!(bb.acquired(), NUM);
        assert_eq!(bb.shared(), NUM);
        assert_eq!(bb.pooled(), 0);

        for i in 1..=NUM {
            bb.release(bufs.pop().unwrap());
            assert_eq!(bb.acquired(), NUM - i);
            assert_eq!(bb.shared(), NUM);
            assert_eq!(bb.pooled(), min(i, MAX_POOLED_BUF));
        }
        assert_eq!(bb.acquired(), 0);
        assert_eq!(bb.shared(), NUM);
        assert_eq!(bb.pooled(), min(NUM, MAX_POOLED_BUF));

        for i in 1..=NUM {
            drop(shared.pop().unwrap());
            let buf = bb.acquire();
            let take_pooled = i <= MAX_POOLED_BUF;
            assert_eq!(bb.acquired(), i);
            assert_eq!(
                bb.shared(),
                if take_pooled {
                    NUM
                } else {
                    NUM + MAX_POOLED_BUF - i
                },
                "i: {i}"
            );
            assert_eq!(
                bb.pooled(),
                min(NUM, MAX_POOLED_BUF).saturating_sub(i),
                "i: {i}"
            );
            bufs.push(buf);
        }
        assert_eq!(bb.acquired(), NUM);
        assert_eq!(bb.shared(), NUM - 1);
        assert_eq!(bb.pooled(), 0);

        for i in 1..=NUM {
            bb.acquire();
            assert_eq!(bb.acquired(), NUM + i);
            assert_eq!(bb.shared(), NUM.saturating_sub(i).saturating_sub(1));
            assert_eq!(bb.pooled(), 0);
        }
        assert_eq!(bb.acquired(), 2 * NUM);
        assert_eq!(bb.shared(), 0);
        assert_eq!(bb.pooled(), 0);
    }

    #[test]
    fn buffers_max_pooled() {
        let mut bb = Buffers::new();
        assert_eq!(bb.acquired(), 0);
        assert_eq!(bb.pooled(), INIT_POOLED_BUF);
        assert_eq!(bb.shared(), 0);

        const NUM: usize = MAX_POOLED_BUF + 1;
        let mut bufs = Vec::with_capacity(NUM);

        for i in 1..=NUM {
            let buf = bb.acquire();
            assert_eq!(bb.acquired(), i);
            assert_eq!(bb.shared(), 0);
            assert_eq!(bb.pooled(), INIT_POOLED_BUF.saturating_sub(i));
            bufs.push(buf);
        }
        assert_eq!(bb.acquired(), NUM);
        assert_eq!(bb.shared(), 0);
        assert_eq!(bb.pooled(), 0);

        let mut shared = Vec::with_capacity(NUM);
        for i in 1..=NUM {
            let arc = bb.release_to_share(bufs.pop().unwrap());
            assert_eq!(bb.acquired(), NUM - i);
            assert_eq!(bb.shared(), i);
            assert_eq!(bb.pooled(), 0);
            shared.push(arc);
        }
        assert_eq!(bb.acquired(), 0);
        assert_eq!(bb.shared(), NUM);
        assert_eq!(bb.pooled(), 0);

        for i in 1..=NUM {
            drop(shared.pop().unwrap());
            let buf = bb.acquire();
            assert_eq!(bb.acquired(), i);
            assert_eq!(bb.shared(), NUM - i);
            assert_eq!(bb.pooled(), 0);
            bufs.push(buf);
        }
        assert_eq!(bb.acquired(), NUM);
        assert_eq!(bb.shared(), 0);
        assert_eq!(bb.pooled(), 0);

        for i in 1..=NUM {
            bb.release(bufs.pop().unwrap());
            assert_eq!(bb.acquired(), NUM - i);
            assert_eq!(bb.shared(), 0);
            assert_eq!(bb.pooled(), min(MAX_POOLED_BUF, i));
        }
        assert_eq!(bb.acquired(), 0);
        assert_eq!(bb.shared(), 0);
        assert_eq!(bb.pooled(), min(MAX_POOLED_BUF, NUM));
    }
}
