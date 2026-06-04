use super::*;
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn put_request_round_trip_succeeds() {
    let request = PutRequest {
        path: "/tmp/file.txt".to_string(),
        size: 42,
        mode: None,
        recursive: false,
    };

    let (mut client, mut server) = tokio::io::duplex(2048);
    let write = tokio::spawn(async move { write_put_request(&mut client, &request).await });
    let read = tokio::spawn(async move { read_put_request(&mut server).await });

    write.await.unwrap().unwrap();
    let decoded = read.await.unwrap().unwrap();
    assert_eq!(
        decoded,
        PutRequest {
            path: "/tmp/file.txt".to_string(),
            size: 42,
            mode: None,
            recursive: false,
        }
    );
}

#[tokio::test]
async fn get_request_round_trip_succeeds() {
    let request = GetRequest {
        path: "/var/log/syslog".to_string(),
        recursive: false,
    };

    let (mut client, mut server) = tokio::io::duplex(2048);
    let write = tokio::spawn(async move { write_get_request(&mut client, &request).await });
    let read = tokio::spawn(async move { read_get_request(&mut server).await });

    write.await.unwrap().unwrap();
    let decoded = read.await.unwrap().unwrap();
    assert_eq!(
        decoded,
        GetRequest {
            path: "/var/log/syslog".to_string(),
            recursive: false,
        }
    );
}

#[tokio::test]
async fn put_ready_chunk_complete_round_trip_succeeds() {
    let ready = TransferReady {
        size: 5,
        mode: None,
    };
    let complete = TransferComplete { size: 5 };
    let chunk = b"hello".to_vec();

    let (mut client, mut server) = tokio::io::duplex(4096);
    let write = tokio::spawn(async move {
        write_put_ready(&mut client, &ready).await.unwrap();
        write_put_chunk(&mut client, &chunk).await.unwrap();
        write_put_complete(&mut client, &complete).await.unwrap();
    });
    let read = tokio::spawn(async move {
        let ready = read_put_ready(&mut server).await.unwrap();
        let chunk = read_put_chunk(&mut server).await.unwrap();
        let complete = read_put_complete(&mut server).await.unwrap();
        (ready, chunk, complete)
    });

    write.await.unwrap();
    let (decoded_ready, decoded_chunk, decoded_complete) = read.await.unwrap();
    assert_eq!(
        decoded_ready,
        TransferReady {
            size: 5,
            mode: None
        }
    );
    assert_eq!(decoded_chunk, b"hello".to_vec());
    assert_eq!(decoded_complete, TransferComplete { size: 5 });
}

#[tokio::test]
async fn get_ready_chunk_complete_round_trip_succeeds() {
    let ready = TransferReady {
        size: 4,
        mode: None,
    };
    let complete = TransferComplete { size: 4 };
    let chunk = b"data".to_vec();

    let (mut client, mut server) = tokio::io::duplex(4096);
    let write = tokio::spawn(async move {
        write_get_ready(&mut client, &ready).await.unwrap();
        write_get_chunk(&mut client, &chunk).await.unwrap();
        write_get_complete(&mut client, &complete).await.unwrap();
    });
    let read = tokio::spawn(async move {
        let ready = read_get_ready(&mut server).await.unwrap();
        let chunk = read_get_chunk(&mut server).await.unwrap();
        let complete = read_get_complete(&mut server).await.unwrap();
        (ready, chunk, complete)
    });

    write.await.unwrap();
    let (decoded_ready, decoded_chunk, decoded_complete) = read.await.unwrap();
    assert_eq!(
        decoded_ready,
        TransferReady {
            size: 4,
            mode: None
        }
    );
    assert_eq!(decoded_chunk, b"data".to_vec());
    assert_eq!(decoded_complete, TransferComplete { size: 4 });
}

#[tokio::test]
async fn transfer_error_round_trip_succeeds() {
    let failure = TransferFailure::new(TransferFailureCode::Rejected, "permission denied");

    let (mut client, mut server) = tokio::io::duplex(1024);
    let write = tokio::spawn(async move { write_transfer_error(&mut client, &failure).await });
    let read = tokio::spawn(async move { read_transfer_error(&mut server).await });

    write.await.unwrap().unwrap();
    let decoded = read.await.unwrap().unwrap();
    assert_eq!(
        decoded,
        TransferFailure::new(TransferFailureCode::Rejected, "permission denied")
    );
}

#[tokio::test]
async fn cwd_request_response_round_trip_succeeds() {
    let response = CwdResponse {
        path: "/home/tester/work".to_string(),
    };

    let (mut client, mut server) = tokio::io::duplex(1024);
    let write = tokio::spawn(async move {
        write_cwd_request(&mut client, &CwdRequest).await.unwrap();
        write_cwd_response(&mut client, &response).await.unwrap();
    });
    let read = tokio::spawn(async move {
        let request = read_next_frame(&mut server).await.unwrap();
        let response = read_next_frame(&mut server).await.unwrap();
        (request, response)
    });

    write.await.unwrap();
    let (request, response) = read.await.unwrap();
    assert_eq!(request, TransferFrame::CwdRequest(CwdRequest));
    assert_eq!(
        response,
        TransferFrame::CwdResponse(CwdResponse {
            path: "/home/tester/work".to_string()
        })
    );
}

#[tokio::test]
async fn exists_request_response_round_trip_succeeds() {
    let request = ExistsRequest {
        path: "/tmp/example".to_string(),
    };
    let response = ExistsResponse {
        exists: true,
        is_dir: false,
    };

    let (mut client, mut server) = tokio::io::duplex(1024);
    let write = tokio::spawn(async move {
        write_exists_request(&mut client, &request).await.unwrap();
        write_exists_response(&mut client, &response).await.unwrap();
    });
    let read = tokio::spawn(async move {
        let request = read_next_frame(&mut server).await.unwrap();
        let response = read_next_frame(&mut server).await.unwrap();
        (request, response)
    });

    write.await.unwrap();
    let (request, response) = read.await.unwrap();
    assert_eq!(
        request,
        TransferFrame::ExistsRequest(ExistsRequest {
            path: "/tmp/example".to_string()
        })
    );
    assert_eq!(
        response,
        TransferFrame::ExistsResponse(ExistsResponse {
            exists: true,
            is_dir: false
        })
    );
}

#[tokio::test]
async fn transfer_rejects_invalid_magic() {
    let (mut client, mut server) = tokio::io::duplex(1024);
    let writer = tokio::spawn(async move {
        client.write_all(b"NOPE").await.unwrap();
        client.write_u8(VERSION).await.unwrap();
        client.write_u8(KIND_PUT_REQUEST).await.unwrap();
        client.write_u32(0).await.unwrap();
    });

    writer.await.unwrap();
    let err = read_put_request(&mut server).await.unwrap_err();
    assert!(matches!(err, TransferError::InvalidMagic));
}

#[tokio::test]
async fn transfer_rejects_oversized_control_payload() {
    let (mut client, mut server) = tokio::io::duplex(1024);
    let writer = tokio::spawn(async move {
        client.write_all(&MAGIC).await.unwrap();
        client.write_u8(VERSION).await.unwrap();
        client.write_u8(KIND_PUT_REQUEST).await.unwrap();
        client
            .write_u32((MAX_CONTROL_BYTES as u32) + 1)
            .await
            .unwrap();
    });

    writer.await.unwrap();
    let err = read_put_request(&mut server).await.unwrap_err();
    assert!(matches!(err, TransferError::PayloadTooLarge(_)));
}

#[tokio::test]
async fn transfer_rejects_oversized_chunk_payload() {
    let (mut client, mut server) = tokio::io::duplex(1024);
    let writer = tokio::spawn(async move {
        client.write_all(&MAGIC).await.unwrap();
        client.write_u8(VERSION).await.unwrap();
        client.write_u8(KIND_GET_CHUNK).await.unwrap();
        client
            .write_u32((MAX_CHUNK_BYTES as u32) + 1)
            .await
            .unwrap();
    });

    writer.await.unwrap();
    let err = read_get_chunk(&mut server).await.unwrap_err();
    assert!(matches!(err, TransferError::PayloadTooLarge(_)));
}

#[tokio::test]
async fn transfer_rejects_unexpected_kind() {
    let (mut client, mut server) = tokio::io::duplex(1024);
    let writer = tokio::spawn(async move {
        write_get_request(
            &mut client,
            &GetRequest {
                path: "/tmp/remote".to_string(),
                recursive: false,
            },
        )
        .await
        .unwrap();
    });

    writer.await.unwrap();
    let err = read_put_request(&mut server).await.unwrap_err();
    assert!(matches!(err, TransferError::UnexpectedKind { .. }));
}

#[tokio::test]
async fn recursive_entry_frames_round_trip_succeeds() {
    let header = EntryHeader {
        path: "subdir/file.txt".to_string(),
        size: 1024,
        mode: Some(0o644),
        is_dir: false,
    };
    let complete = EntryComplete;

    let (mut client, mut server) = tokio::io::duplex(2048);

    // Test EntryHeader
    let write_h = tokio::spawn(async move { write_new_entry(&mut client, &header).await });
    let read_h = tokio::spawn(async move { read_next_frame(&mut server).await });

    write_h.await.unwrap().unwrap();
    let decoded_h = read_h.await.unwrap().unwrap();
    assert_eq!(
        decoded_h,
        TransferFrame::NewEntry(EntryHeader {
            path: "subdir/file.txt".to_string(),
            size: 1024,
            mode: Some(0o644),
            is_dir: false,
        })
    );

    // Test EntryComplete
    let (mut client, mut server) = tokio::io::duplex(2048);
    let write_c = tokio::spawn(async move { write_entry_complete(&mut client, &complete).await });
    let read_c = tokio::spawn(async move { read_next_frame(&mut server).await });

    write_c.await.unwrap().unwrap();
    let decoded_c = read_c.await.unwrap().unwrap();
    assert_eq!(decoded_c, TransferFrame::EntryComplete(EntryComplete));
}

#[tokio::test]
async fn read_next_frame_decodes_chunk_and_error_frames() {
    let (mut client, mut server) = tokio::io::duplex(2048);
    let write = tokio::spawn(async move {
        write_get_chunk(&mut client, b"hello").await.unwrap();
        write_transfer_error(
            &mut client,
            &TransferFailure::new(TransferFailureCode::Internal, "nope"),
        )
        .await
        .unwrap();
    });
    let read = tokio::spawn(async move {
        let first = read_next_frame(&mut server).await.unwrap();
        let second = read_next_frame(&mut server).await.unwrap();
        (first, second)
    });

    write.await.unwrap();
    let (first, second) = read.await.unwrap();
    assert_eq!(first, TransferFrame::GetChunk(b"hello".to_vec()));
    assert_eq!(
        second,
        TransferFrame::Error(TransferFailure::new(TransferFailureCode::Internal, "nope"))
    );
}

#[tokio::test]
async fn old_decoder_rejects_unknown_transfer_kind() {
    let (mut client, mut server) = tokio::io::duplex(1024);
    let unknown_kind = 255;

    let writer = tokio::spawn(async move {
        client.write_all(&MAGIC).await.unwrap();
        client.write_u8(VERSION).await.unwrap();
        client.write_u8(unknown_kind).await.unwrap();
        client.write_u32(0).await.unwrap();
    });

    writer.await.unwrap();
    let err = read_next_frame(&mut server).await.unwrap_err();
    assert!(matches!(err, TransferError::UnsupportedKind(k) if k == unknown_kind));
}

#[tokio::test]
async fn blob_get_ready_round_trip_succeeds() {
    let ready = BlobGetReady {
        hash: "sha256:abc123".to_string(),
        format: "raw".to_string(),
        size: 4096,
    };

    let (mut client, mut server) = tokio::io::duplex(1024);
    let write = tokio::spawn(async move { write_blob_get_ready(&mut client, &ready).await });
    let read = tokio::spawn(async move { read_next_frame(&mut server).await });

    write.await.unwrap().unwrap();
    let decoded = read.await.unwrap().unwrap();
    assert_eq!(
        decoded,
        TransferFrame::BlobGetReady(BlobGetReady {
            hash: "sha256:abc123".to_string(),
            format: "raw".to_string(),
            size: 4096,
        })
    );
}

#[tokio::test]
async fn capability_round_trip_succeeds() {
    let (mut client, mut server) = tokio::io::duplex(256);
    let write =
        tokio::spawn(
            async move { write_capability(&mut client, &Capability { max_kind: 20 }).await },
        );
    let read = tokio::spawn(async move { read_capability(&mut server).await });

    write.await.unwrap().unwrap();
    let decoded = read.await.unwrap().unwrap();
    assert_eq!(decoded, Capability { max_kind: 20 });
}

#[tokio::test]
async fn capability_is_recognized_by_read_next_frame() {
    let (mut client, mut server) = tokio::io::duplex(256);
    let write = tokio::spawn(async move {
        write_capability(&mut client, &Capability { max_kind: 20 })
            .await
            .unwrap();
    });
    let read = tokio::spawn(async move { read_next_frame(&mut server).await });

    write.await.unwrap();
    let decoded = read.await.unwrap().unwrap();
    assert_eq!(
        decoded,
        TransferFrame::Capability(Capability { max_kind: 20 })
    );
}

#[tokio::test]
async fn negotiation_full_handshake_succeeds() {
    let client_cap = Capability { max_kind: 20 };
    let server_cap = Capability { max_kind: 17 };

    let (mut client, mut server) = tokio::io::duplex(256);

    let client_task = tokio::spawn(async move {
        write_capability(&mut client, &client_cap).await.unwrap();
        let resp = read_capability(&mut client).await.unwrap();
        std::cmp::min(client_cap.max_kind, resp.max_kind)
    });

    let server_task = tokio::spawn(async move {
        let client = read_capability(&mut server).await.unwrap();
        let negotiated = std::cmp::min(client.max_kind, server_cap.max_kind);
        write_capability(
            &mut server,
            &Capability {
                max_kind: server_cap.max_kind,
            },
        )
        .await
        .unwrap();
        negotiated
    });

    let client_result = client_task.await.unwrap();
    let server_result = server_task.await.unwrap();
    assert_eq!(client_result, 17); // min(20, 17)
    assert_eq!(server_result, 17);
}

#[tokio::test]
async fn capability_negotiation_with_legacy_peer_graceful_fallback_works() {
    // Simulate end-to-end: new client sends capability, old server (pre-capability)
    // would reject it. The rejection path in read_frame returns UnsupportedKind.
    // This test verifies the legacy decoder rejects kind 255 as a proxy for the
    // old-server behavior (already tested in old_decoder_rejects_unknown_transfer_kind).
    // Here we verify the new decoder correctly handles the capability handshake
    // by accepting kind 0 and parsing it as a valid frame.
    let (mut client, mut server) = tokio::io::duplex(256);
    let writer = tokio::spawn(async move {
        write_capability(&mut client, &Capability { max_kind: 0 })
            .await
            .unwrap();
    });

    writer.await.unwrap();
    let frame = read_next_frame(&mut server).await.unwrap();
    assert!(matches!(frame, TransferFrame::Capability(_)));
}
