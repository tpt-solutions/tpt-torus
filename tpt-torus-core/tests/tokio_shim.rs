#![cfg(feature = "tokio")]

//! Integration tests for the tokio `AsyncRead`/`AsyncWrite` shim.
//!
//! Uses an in-memory mock backend so the shim can be exercised on any platform
//! without real file/socket I/O.

use std::sync::Arc;
use std::sync::Mutex;

use tpt_torus_core::async_api::TorusAsync;
use tpt_torus_core::async_tokio::{TorusAsyncReader, TorusAsyncWriter};
use tpt_torus_core::backend::Backend;
use tpt_torus_core::flow::Flow;
use tpt_torus_core::operation::Operation;
use tpt_torus_core::{Torus, TorusResult};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct MockBackend {
    completions: Mutex<Vec<TorusResult>>,
}

impl Backend for MockBackend {
    fn submit(&self, flows: &[Flow]) -> tpt_torus_core::error::Result<usize> {
        for flow in flows {
            let (bytes, user_data) = match flow.operation() {
                Operation::Read { buf, len, .. } => {
                    // Fill the buffer with a recognizable pattern.
                    unsafe {
                        for i in 0..*len {
                            *buf.add(i) = (i % 251) as u8;
                        }
                    }
                    (*len as i64, flow.user_data())
                }
                Operation::Write { len, .. } => (*len as i64, flow.user_data()),
                other => panic!("tokio_shim test got unexpected op: {:?}", other),
            };
            self.completions
                .lock()
                .unwrap()
                .push(TorusResult::new(bytes, user_data));
        }
        Ok(flows.len())
    }

    fn reap(&self, results: &mut Vec<TorusResult>) -> tpt_torus_core::error::Result<usize> {
        let mut cq = self.completions.lock().unwrap();
        let before = results.len();
        results.append(&mut cq);
        Ok(results.len() - before)
    }

    fn wait(&self, _timeout_us: u64) -> tpt_torus_core::error::Result<()> {
        Ok(())
    }

    fn in_flight(&self) -> u32 {
        0
    }
}

fn make_async() -> TorusAsync {
    let torus = Arc::new(
        Torus::new(
            256,
            Box::new(MockBackend {
                completions: Mutex::new(Vec::new()),
            }),
        )
        .unwrap(),
    );
    TorusAsync::from_torus(torus)
}

#[tokio::test]
async fn tokio_reader_fills_buffer() {
    let torus = Arc::new(make_async());
    let mut reader = TorusAsyncReader::new(torus, 0);

    let mut buf = [0u8; 16];
    reader.read_exact(&mut buf).await.unwrap();
    assert_eq!(buf[0], 0);
    assert_eq!(buf[1], 1);
    assert_eq!(buf[15], 15);
}

#[tokio::test]
async fn tokio_writer_writes_all() {
    let torus = Arc::new(make_async());
    let mut writer = TorusAsyncWriter::new(torus, 0);

    let data = b"hello from the tokio shim";
    let n = writer.write(data).await.unwrap();
    assert_eq!(n, data.len());
}
