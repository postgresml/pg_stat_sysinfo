use std::default::Default;
use std::io::BufReader;
use std::marker::PhantomData;

use anyhow::anyhow;
use pgrx::*;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::crate_info::CRATE;

const ONE_KB: usize = 1024;
const PAGE_SIZE: usize = 256 * ONE_KB;
// const PAGE_SIZE: usize = 1 * ONE_KB;
const PAGES: usize = 5;

/**
  The ring buffer is statically allocated, broken into pages of a fixed size.
  Objects are serialized into to the buffer. When the last page is full,
  the first page is cleared and all the pages are rotated so that the first
  page is last, the last page is second to last, and so on.
*/
pub type ShmemRingBuffer<T> = ShmemBackedSerdeRingBuffer<PAGE_SIZE, PAGES, T>;

#[derive(Clone, Debug)]
pub struct ShmemBackedSerdeRingBuffer<
    const N: usize,
    const M: usize,
    T: Serialize + DeserializeOwned,
> {
    data: heapless::Vec<heapless::Vec<u8, N>, M>,
    counts: heapless::Vec<usize, M>,
    _phantom: PhantomData<T>,
}

impl<const N: usize, const M: usize, T: Serialize + DeserializeOwned>
    ShmemBackedSerdeRingBuffer<N, M, T>
{
    /**
      Writes objects as JSON to the ring buffer.
    */
    pub fn write(&mut self, item: T) -> anyhow::Result<()> {
        let last = self.data.last().expect("No cache pages?");
        let (used, capacity) = (last.len(), last.capacity());

        let mut encoded = Vec::new();
        serde_bare::to_writer(&mut encoded, &item)?;

        if capacity / 2 < encoded.len() {
            // Although it could fit, objects should never be this big;
            // something is wrong; and if they get too much bigger, it will
            // prevent the rotation from working.
            return Err(anyhow!(
                "Object is too large ({} bytes) for rings of {} bytes.",
                encoded.len(),
                capacity
            ));
        }

        if (used + encoded.len() + 2) >= capacity {
            self.data.rotate_left(1);
            self.counts.rotate_left(1);

            if !self.data.last().expect("No cache pages?").is_empty() {
                let was = self.stats();
                self.data.last_mut().expect("No cache pages?").clear();
                *self.counts.last_mut().expect("No counts?") = 0;
                let is = self.stats();
                debug1!(
                    "{}: Rotated buffer -- cleared one page. \
                     usage: {} -> {} | items: {} -> {} | bytes/item: {} -> {}",
                    CRATE,
                    bytesize::to_string(was.bytes_used as u64, true),
                    bytesize::to_string(is.bytes_used as u64, true),
                    was.items,
                    is.items,
                    was.item_average_bytes,
                    is.item_average_bytes,
                );
            }
        }

        let err = |_| anyhow!("Failure to extend: heapless::Vec");
        let real_last = self.data.last_mut().expect("No cache pages?");
        real_last.extend_from_slice(&encoded).map_err(err)?;
        *self.counts.last_mut().expect("No counts?") += 1;

        Ok(())
    }

    pub fn read(&self) -> Vec<T> {
        let mut results: Vec<T> = vec![];

        for page in &self.data {
            let mut reader = BufReader::new(page.as_slice());
            let mut error: Option<_> = None;

            while error.is_none() {
                match serde_bare::from_reader(&mut reader) {
                    Ok(item) => {
                        results.push(item);
                    }
                    Err(err) => {
                        error = Some(err);
                        // TODO: Distinguish real errors and the "error" of
                        // reaching the end of the buffer.
                    }
                }
            }
        }

        results
    }

    pub fn stats(&self) -> BufferSummary {
        let items = self.counts.iter().sum();
        let bytes_used = self.data.iter().map(|v| v.len()).sum();
        let item_average_bytes = (bytes_used as f64 / items as f64) as f32;

        BufferSummary {
            items,
            bytes_used,
            item_average_bytes,
        }
    }
}

impl<const N: usize, const M: usize, T: Serialize + DeserializeOwned> Default
    for ShmemBackedSerdeRingBuffer<N, M, T>
{
    fn default() -> Self {
        let _phantom = PhantomData;
        let mut data = heapless::Vec::default();
        let mut counts = heapless::Vec::default();
        for _ in 0..data.capacity() {
            let msg = "Pushing too many elements here is impossible.";
            data.push(heapless::Vec::default()).expect(msg);
            counts.push(0).expect(msg);
        }
        ShmemBackedSerdeRingBuffer {
            data,
            counts,
            _phantom,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct BufferSummary {
    pub items: usize,
    pub bytes_used: usize,
    pub item_average_bytes: f32,
}
