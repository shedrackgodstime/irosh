use super::*;
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn metadata_round_trip_succeeds() {
    let metadata = PeerMetadata {
        hostname: "host".to_string(),
        user: "user".to_string(),
        os: "linux".to_string(),
    };
    let expected = metadata.clone();

    let (mut client, mut server) = tokio::io::duplex(1024);
    let write = tokio::spawn(async move { write_metadata(&mut client, &metadata).await });
    let read = tokio::spawn(async move { read_metadata(&mut server).await });

    write.await.unwrap().unwrap();
    let decoded = read.await.unwrap().unwrap();

    assert_eq!(decoded, expected);
}

#[tokio::test]
async fn metadata_request_response_round_trip_succeeds() {
    let metadata = PeerMetadata {
        hostname: "host".to_string(),
        user: "user".to_string(),
        os: "linux".to_string(),
    };

    let (mut client, mut server) = tokio::io::duplex(1024);
    let write = tokio::spawn(async move {
        write_metadata_request(&mut client).await.unwrap();
        write_metadata(&mut client, &metadata).await.unwrap();
    });
    let read = tokio::spawn(async move {
        read_metadata_request(&mut server).await.unwrap();
        read_metadata(&mut server).await.unwrap()
    });

    write.await.unwrap();
    let decoded = read.await.unwrap();
    assert_eq!(
        decoded,
        PeerMetadata {
            hostname: "host".to_string(),
            user: "user".to_string(),
            os: "linux".to_string(),
        }
    );
}

#[tokio::test]
async fn metadata_rejects_invalid_magic() {
    let (mut client, mut server) = tokio::io::duplex(1024);
    let writer = tokio::spawn(async move {
        client.write_all(b"NOPE").await.unwrap();
        client.write_u8(VERSION).await.unwrap();
        client.write_u8(KIND_PEER_METADATA).await.unwrap();
        client.write_u32(0).await.unwrap();
    });

    writer.await.unwrap();
    let err = read_metadata(&mut server).await.unwrap_err();
    assert!(matches!(err, MetadataError::InvalidMagic));
}

#[tokio::test]
async fn metadata_rejects_oversized_payload() {
    let (mut client, mut server) = tokio::io::duplex(1024);
    let writer = tokio::spawn(async move {
        client.write_all(&MAGIC).await.unwrap();
        client.write_u8(VERSION).await.unwrap();
        client.write_u8(KIND_PEER_METADATA).await.unwrap();
        client
            .write_u32((MAX_METADATA_BYTES as u32) + 1)
            .await
            .unwrap();
    });

    writer.await.unwrap();
    let err = read_metadata(&mut server).await.unwrap_err();
    assert!(matches!(err, MetadataError::PayloadTooLarge(_)));
}

#[tokio::test]
async fn metadata_rejects_unexpected_frame_kind() {
    let (mut client, mut server) = tokio::io::duplex(1024);
    let writer = tokio::spawn(async move {
        write_metadata_request(&mut client).await.unwrap();
    });

    writer.await.unwrap();
    let err = read_metadata(&mut server).await.unwrap_err();
    assert!(matches!(err, MetadataError::UnexpectedKind { .. }));
}
