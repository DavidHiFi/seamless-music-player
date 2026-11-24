//! A single-producer single-consumer ring buffer for interleaved f32 frames.
//! Lock-free: the decoder thread writes, the device callback reads, and the
//! two only meet in their own atomic cursors. Capacity is a power of two.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct Ring {
    // Written by the producer thread, read by the device callback. The
    // atomics below are the synchronisation; the cell only hands out the
    // memory.
    buf: UnsafeCell<Box<[f32]>>,
    mask: usize,
    channels: usize,
    write_pos: AtomicUsize,
    read_pos: AtomicUsize,
}

// One producer and one consumer, ordered by the Acquire/Release cursors.
unsafe impl Sync for Ring {}

impl Ring {
    pub fn new(capacity_frames: usize, channels: usize) -> Ring {
        let cap = capacity_frames.max(16).next_power_of_two();
        Ring {
            buf: UnsafeCell::new(vec![0.0; cap * channels].into_boxed_slice()),
            mask: cap - 1,
            channels: channels.max(1),
            write_pos: AtomicUsize::new(0),
            read_pos: AtomicUsize::new(0),
        }
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn capacity_frames(&self) -> usize {
        self.mask + 1
    }

    fn used(&self) -> usize {
        self.write_pos.load(Ordering::Acquire) - self.read_pos.load(Ordering::Acquire)
    }

    /// Frames free for the producer.
    pub fn free_frames(&self) -> usize {
        self.capacity_frames() - self.used()
    }

    /// Frames waiting for the consumer.
    pub fn used_frames(&self) -> usize {
        self.used()
    }

    /// Write interleaved frames from src. Returns the frames written, which
    /// is never more than what fits.
    pub fn write(&self, src: &[f32]) -> usize {
        let ch = self.channels;
        let mut frames = src.len() / ch;
        let free = self.free_frames();
        if frames > free {
            frames = free;
        }
        let mut w = self.write_pos.load(Ordering::Relaxed) & self.mask;
        let mut off = 0usize;
        let buf = unsafe { &mut *self.buf.get() };
        for _ in 0..frames {
            let base = w * ch;
            buf[base..base + ch].copy_from_slice(&src[off..off + ch]);
            off += ch;
            w = (w + 1) & self.mask;
        }
        if frames > 0 {
            self.write_pos.fetch_add(frames, Ordering::Release);
        }
        frames
    }

    /// Read interleaved frames into out (which holds out.len()/ch frames).
    /// Returns the frames read; the rest of out is left untouched, so the
    /// caller zeroes it for silence.
    pub fn read(&self, out: &mut [f32]) -> usize {
        let ch = self.channels;
        let mut frames = out.len() / ch;
        let used = self.used();
        if frames > used {
            frames = used;
        }
        let mut r = self.read_pos.load(Ordering::Relaxed) & self.mask;
        let mut off = 0usize;
        let buf = unsafe { &*self.buf.get() };
        for _ in 0..frames {
            let base = r * ch;
            out[off..off + ch].copy_from_slice(&buf[base..base + ch]);
            off += ch;
            r = (r + 1) & self.mask;
        }
        if frames > 0 {
            self.read_pos.fetch_add(frames, Ordering::Release);
        }
        frames
    }

    /// Drop every buffered frame. Only safe when both sides are quiet: the
    /// engine swaps in a fresh ring for cuts instead of calling this.
    #[allow(dead_code)]
    pub fn clear(&self) {
        self.write_pos.store(0, Ordering::Release);
        self.read_pos.store(0, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_roundtrip() {
        let r = Ring::new(1024, 2);
        let src: Vec<f32> = (0..500 * 2).map(|i| i as f32 * 0.001).collect();
        assert_eq!(r.write(&src), 500);
        let mut out = vec![0.0f32; 300 * 2];
        assert_eq!(r.read(&mut out), 300);
        assert_eq!(out[0], 0.0);
        assert_eq!(out[599], 299.0 * 0.001);
        let mut out2 = vec![0.0f32; 300 * 2];
        assert_eq!(r.read(&mut out2), 200);
        assert_eq!(out2[399], 499.0 * 0.001);
        assert_eq!(r.used_frames(), 0);
    }

    #[test]
    fn ring_wraps() {
        let r = Ring::new(128, 1);
        let one = vec![0.5f32; 100];
        for _ in 0..7 {
            r.write(&one);
            let mut out = vec![0.0f32; 100];
            assert_eq!(r.read(&mut out), 100);
        }
    }

    #[test]
    fn ring_never_overflows() {
        let r = Ring::new(64, 2);
        let big = vec![1.0f32; 1000 * 2];
        let w1 = r.write(&big);
        let w2 = r.write(&big);
        assert!(w1 + w2 <= 63);
    }
}
