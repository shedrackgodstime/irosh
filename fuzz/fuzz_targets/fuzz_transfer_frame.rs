#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        use irosh::transport::transfer::*;
        let mut reader = data;
        let _ = read_next_frame(&mut reader).await;
        let mut reader = data;
        let _ = read_put_request(&mut reader).await;
        let mut reader = data;
        let _ = read_get_request(&mut reader).await;
        let mut reader = data;
        let _ = read_put_ready(&mut reader).await;
        let mut reader = data;
        let _ = read_put_chunk(&mut reader).await;
        let mut reader = data;
        let _ = read_get_chunk(&mut reader).await;
        let mut reader = data;
        let _ = read_transfer_error(&mut reader).await;
        let mut reader = data;
        let _ = read_exists_request(&mut reader).await;
        let mut reader = data;
        let _ = read_exists_response(&mut reader).await;
    });
});
