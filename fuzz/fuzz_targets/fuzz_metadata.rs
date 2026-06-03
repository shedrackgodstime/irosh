#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        use irosh::transport::metadata::*;
        let mut reader = data;
        let _ = read_metadata_request(&mut reader).await;
        let mut reader = data;
        let _ = read_metadata(&mut reader).await;
    });
});
